param(
    [string]$SharedRoot = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',
    [string]$ToolchainBin = '',
    [string]$Python = 'python',
    [switch]$SkipDifferential
)
$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$evidenceRoot = Join-Path $repoRoot '.artifacts/wallet-hashes-evidence'
New-Item -ItemType Directory -Force -Path $evidenceRoot | Out-Null
if (-not $ToolchainBin) {
    $ToolchainBin = Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin'
}
$rustc = Join-Path $ToolchainBin rustc.exe
$rustfmt = Join-Path $ToolchainBin rustfmt.exe
$clippy = Join-Path $ToolchainBin clippy-driver.exe
$source = Join-Path $repoRoot 'mcw/src/wallet_hashes.rs'
$encoding = Join-Path $repoRoot 'mcw/src/bitcoin_encoding.rs'
$tests = Join-Path $PSScriptRoot 'wallet_hashes_conformance.rs'
$fixture = Join-Path $PSScriptRoot 'wallet_hashes_fixtures/vectors.tsv'
$manifest = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'wallet_hashes_fixtures/manifest.json') -Raw | ConvertFrom-Json
function File-Hash([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
if ((File-Hash $fixture) -ne $manifest.fixture_sha256) { throw 'Primary vector fixture hash mismatch' }
$sourceHashes = [ordered]@{wallet_hashes = File-Hash $source; bitcoin_encoding = File-Hash $encoding; conformance = File-Hash $tests; fixtures = File-Hash $fixture}
if ((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory -lt 2097152) {
    Write-Output 'BUILD_DEFERRED: less than 2 GiB free'; exit 3
}
$slotHandle = $null
foreach ($slot in 1, 2) {
    try {
        $slotHandle = [IO.File]::Open((Join-Path $SharedRoot ".artifacts/mcw-coordination/build-slot-$slot.lock"),
            [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        break
    } catch [IO.IOException] { }
}
if (-not $slotHandle) { Write-Output 'BUILD_SLOTS_BUSY'; exit 3 }
$oldPath = $env:PATH
$oldLib = $env:LIB
$oldJobs = $env:CARGO_BUILD_JOBS
try {
    $compilerVersion = & $rustc --version
    if ($LASTEXITCODE -ne 0 -or $compilerVersion -notlike 'rustc 1.99.0 *') { throw 'Rust 1.99.0 required' }
    $harness = 'extern crate self as mcw;' + [Environment]::NewLine +
        '#[path="' + $encoding.Replace('\', '/') + '"] pub mod bitcoin_encoding;' + [Environment]::NewLine +
        '#[path="' + $source.Replace('\', '/') + '"] pub mod wallet_hashes;' + [Environment]::NewLine +
        '#[path="' + $tests.Replace('\', '/') + '"] mod conformance;'
    $harnessPath = Join-Path $evidenceRoot 'actual-source-tests.rs'
    [IO.File]::WriteAllText($harnessPath, $harness)
    & $rustfmt --edition 2024 --check $source $tests
    if ($LASTEXITCODE -ne 0) { throw 'Rust formatting check failed' }
    & $clippy --edition=2024 --test --emit=metadata -D warnings $harnessPath -o (Join-Path $evidenceRoot 'clippy-tests.rmeta')
    if ($LASTEXITCODE -ne 0) { throw 'Clippy -D warnings failed' }
    # Existing MSVC/SDK libraries are test tooling only. The ignored harness uses
    # static CRT to run independently of an installed redistributable. It is never
    # an mcw shipping binary or evidence of production packaging.
    $linker = Get-ChildItem 'C:\Program Files\Microsoft Visual Studio\*\*\VC\Tools\MSVC\*\bin\Hostx64\x64\link.exe' |
        Sort-Object FullName -Descending | Select-Object -First 1
    $kernelLib = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\Lib\*\um\x64\kernel32.lib' |
        Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $linker -or -not $kernelLib) { throw 'Existing MSVC linker and SDK libraries required' }
    $msvcRoot = [IO.Path]::GetFullPath((Join-Path $linker.Directory.FullName '../../..'))
    $sdkRoot = [IO.Path]::GetFullPath((Join-Path $kernelLib.Directory.FullName '../..'))
    $nativeLibraries = @((Join-Path $msvcRoot 'lib/onecore/x64'), (Join-Path $sdkRoot 'ucrt/x64'), (Join-Path $sdkRoot 'um/x64'))
    $env:PATH = $linker.Directory.FullName + ';' + $env:PATH
    $env:LIB = $nativeLibraries -join ';'
    $env:CARGO_BUILD_JOBS = '1'
    $runs = @()
    foreach ($profile in 'debug', 'optimized') {
        $binary = Join-Path $evidenceRoot "wallet-hashes-$profile.exe"
        $arguments = @('--edition=2024', '--test', '-D', 'warnings', '-C', 'overflow-checks=yes',
            '-C', 'codegen-units=1', '-C', 'target-feature=+crt-static', $harnessPath, '-o', $binary)
        if ($profile -eq 'optimized') { $arguments += @('-C', 'opt-level=3') }
        & $rustc @arguments
        if ($LASTEXITCODE -ne 0) { throw "$profile harness compilation failed" }
        $log = Join-Path $evidenceRoot "$profile-tests.txt"
        & $binary --test-threads=1 2>&1 | Tee-Object -FilePath $log
        if ($LASTEXITCODE -ne 0) { throw "$profile conformance failed" }
        $result = Select-String -LiteralPath $log -Pattern 'test result: ok\. (\d+) passed; (\d+) failed'
        if (-not $result) { throw 'Test result counts missing' }
        $runs += [ordered]@{profile = $profile; passed = [int]$result.Matches[0].Groups[1].Value; failed = [int]$result.Matches[0].Groups[2].Value; log = $log; binary_sha256 = File-Hash $binary}
    }
    if (-not $SkipDifferential) {
        $pythonArguments = @((Join-Path $PSScriptRoot 'wallet_hashes_reference.py'), '--rustc', $rustc, '--work-directory', $evidenceRoot)
        & $Python @pythonArguments
        if ($LASTEXITCODE -ne 0) { throw 'Python differential verification failed' }
    }
    $libraryHarness = Join-Path $evidenceRoot 'actual-source-library.rs'
    [IO.File]::WriteAllText($libraryHarness, '#![forbid(unsafe_code)]' + [Environment]::NewLine +
        '#[path="' + $encoding.Replace('\', '/') + '"] pub mod bitcoin_encoding;' + [Environment]::NewLine +
        '#[path="' + $source.Replace('\', '/') + '"] pub mod wallet_hashes;')
    & $clippy --edition=2024 --crate-type=lib --emit=metadata -D warnings $libraryHarness -o (Join-Path $evidenceRoot 'clippy-library.rmeta')
    if ($LASTEXITCODE -ne 0) { throw 'Library Clippy failed' }
    $sysroot = & $rustc --print sysroot
    $targets = @()
    foreach ($target in 'x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu', 'x86_64-apple-darwin', 'aarch64-apple-darwin') {
        if (Test-Path -LiteralPath (Join-Path $sysroot "lib/rustlib/$target/lib")) {
            & $rustc --edition=2024 --crate-type=lib --emit=metadata --target $target -D warnings $libraryHarness -o (Join-Path $evidenceRoot "$target.rmeta")
            if ($LASTEXITCODE -ne 0) { throw "$target metadata compilation failed" }
            $targets += [ordered]@{target = $target; state = 'metadata_pass'; runtime = 'not_exercised_by_metadata'}
        } else {
            $targets += [ordered]@{target = $target; state = 'unavailable'; reason = 'target standard library not installed'}
        }
    }
    $targets | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $evidenceRoot 'target-checks.json') -Encoding utf8NoBOM
    foreach ($key in $sourceHashes.Keys) {
        $path = switch ($key) {'wallet_hashes' {$source} 'bitcoin_encoding' {$encoding} 'conformance' {$tests} 'fixtures' {$fixture}}
        if ((File-Hash $path) -ne $sourceHashes[$key]) { throw 'Source changed during verification' }
    }
    $summary = [ordered]@{compiler = $compilerVersion; edition = 2024; sources = $sourceHashes; primary_vectors = $manifest.vectors;
        runs = $runs; clippy = 'library and tests passed with -D warnings'; targets = $targets; production_release = $false;
        note = 'Actual source harnesses only. No Cargo package, manifest edit, production caller, wallet or key state migration, or shipping executable.'}
    $summary | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath (Join-Path $evidenceRoot 'verification.json') -Encoding utf8NoBOM
    $summary | ConvertTo-Json -Depth 7 -Compress | Write-Output
} finally {
    $env:PATH = $oldPath; $env:LIB = $oldLib; $env:CARGO_BUILD_JOBS = $oldJobs
    $slotHandle.Dispose()
}
