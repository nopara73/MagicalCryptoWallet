param(
    [string]$SharedRoot = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',
    [string]$EncodingSource = '',
    [string]$Rustc = ''
)
$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$evidenceRoot = Join-Path $repoRoot '.artifacts/bitcoin-wire'
$harnessRoot = Join-Path $evidenceRoot 'harness'
New-Item -ItemType Directory -Force -Path $harnessRoot | Out-Null
if (-not $EncodingSource) {
    $EncodingSource = Join-Path $repoRoot 'mcw/src/bitcoin_encoding.rs'
    if (-not (Test-Path -LiteralPath $EncodingSource)) {
        $EncodingSource = Join-Path $SharedRoot '.artifacts/mcw-bitcoin-encoding/mcw/src/bitcoin_encoding.rs'
    }
}
$EncodingSource = [IO.Path]::GetFullPath($EncodingSource)
if (-not $Rustc) {
    $Rustc = Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin/rustc.exe'
}
$wirePath = Join-Path $repoRoot 'mcw/src/bitcoin_wire.rs'
$testPath = Join-Path $repoRoot 'mcw/tests/bitcoin_wire_conformance.rs'
$fixturePath = Join-Path $repoRoot 'mcw/tests/bitcoin_wire_fixtures/reference.tsv'
$manifest = Get-Content -LiteralPath (Join-Path $repoRoot 'mcw/tests/bitcoin_wire_fixtures/manifest.json') -Raw | ConvertFrom-Json
function Source-Hash([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
$fixtureHash = Source-Hash $fixturePath
if ($fixtureHash -ne $manifest.fixture_sha256) { throw 'Reference fixture checksum mismatch' }
$sourceHashes = @{
    bitcoin_encoding = Source-Hash $EncodingSource
    bitcoin_wire = Source-Hash $wirePath
    conformance = Source-Hash $testPath
}
$buildHandle = $null
try {
    if ((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory -lt 2097152) {
        throw 'Build deferred: less than 2 GiB free memory'
    }
    foreach ($slot in 1, 2) {
        try {
            $slotPath = Join-Path $SharedRoot ('.artifacts/mcw-coordination/build-slot-' + $slot + '.lock')
            $buildHandle = [IO.File]::Open($slotPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
            break
        } catch [IO.IOException] { }
    }
    if (-not $buildHandle) { throw 'Build deferred: both shared compiler slots occupied' }
    $compilerVersion = & $Rustc --version
    if ($LASTEXITCODE -ne 0 -or $compilerVersion -notlike 'rustc 1.99.0 *') { throw 'Rust 1.99.0 required' }
    $harnessText = 'extern crate self as mcw;' + [Environment]::NewLine +
        '#[path="' + $EncodingSource.Replace('\', '/') + '"] pub mod bitcoin_encoding;' + [Environment]::NewLine +
        '#[path="' + $wirePath.Replace('\', '/') + '"] pub mod bitcoin_wire;' + [Environment]::NewLine +
        '#[path="' + $testPath.Replace('\', '/') + '"] mod conformance;'
    $harnessSource = Join-Path $harnessRoot 'main.rs'
    [IO.File]::WriteAllText($harnessSource, $harnessText)
    $linker = Get-ChildItem -Path 'C:\Program Files\Microsoft Visual Studio\*\*\VC\Tools\MSVC\*\bin\Hostx64\x64\link.exe' |
        Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $linker) { throw 'Existing MSVC linker required for this Windows test harness' }
    $msvcRoot = [IO.Path]::GetFullPath((Join-Path $linker.Directory.FullName '../../..'))
    $sdk = Get-ChildItem -LiteralPath 'C:\Program Files (x86)\Windows Kits\10\Lib' -Directory |
        Sort-Object Name -Descending | Select-Object -First 1
    $env:PATH = $linker.Directory.FullName + ';' + $env:PATH
    $env:LIB = (Join-Path $msvcRoot 'lib/onecore/x64') + ';' +
        (Join-Path $sdk.FullName 'ucrt/x64') + ';' + (Join-Path $sdk.FullName 'um/x64')
    $env:CARGO_BUILD_JOBS = '1'
    $runs = @()
    foreach ($mode in 'debug', 'optimized') {
        $binary = Join-Path $harnessRoot ('wire-' + $mode + '.exe')
        $compilerArguments = @('--edition=2024', '--test', '-D', 'warnings', '-C', 'overflow-checks=yes',
            '-C', 'target-feature=+crt-static', $harnessSource, '-o', $binary)
        if ($mode -eq 'optimized') { $compilerArguments += @('-C', 'opt-level=2') }
        & $Rustc @compilerArguments
        if ($LASTEXITCODE -ne 0) { throw ($mode + ' conformance build failed') }
        $log = Join-Path $evidenceRoot ($mode + '.log')
        & $binary --test-threads=1 2>&1 | Tee-Object -FilePath $log
        if ($LASTEXITCODE -ne 0) { throw ($mode + ' conformance tests failed') }
        $logText = Get-Content -LiteralPath $log -Raw
        if ($logText -notmatch 'test result: ok\. (\d+) passed; 0 failed;') { throw 'Test completion not verified' }
        $runs += @{ mode = $mode; passed = [int]$Matches[1]; failed = 0; log = $log }
    }
    if ((Source-Hash $EncodingSource) -ne $sourceHashes.bitcoin_encoding -or
        (Source-Hash $wirePath) -ne $sourceHashes.bitcoin_wire -or
        (Source-Hash $testPath) -ne $sourceHashes.conformance -or
        (Source-Hash $fixturePath) -ne $fixtureHash) { throw 'Sources changed during verification' }
    $evidence = @{
        compiler = $compilerVersion
        edition = 2024
        platform = 'x86_64-pc-windows-msvc'
        sources = $sourceHashes
        encoding_source = $EncodingSource
        fixture_sha256 = $fixtureHash
        fixtures = [int]$manifest.total
        wire_tests = @(Select-String -LiteralPath $testPath -Pattern '^#\[test\]').Count
        runs = $runs
        production_release = $false
        note = 'Ignored test harness only; static CRT used to run tests with the installed linker. No shipping executable or Cargo package added. Other target runtime/build verification remains with integration.'
    }
    $evidence | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $evidenceRoot 'verification.json') -Encoding utf8
    $evidence | ConvertTo-Json -Depth 8
} finally {
    if ($buildHandle) { $buildHandle.Dispose() }
}
