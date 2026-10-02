param(
    [string]$SharedRoot = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',
    [switch]$SkipDifferential
)
$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$evidenceRoot = Join-Path $repoRoot '.artifacts/bitcoin-script-evidence'
New-Item -ItemType Directory -Force -Path $evidenceRoot | Out-Null
$toolsBin = Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin'
$rustc = Join-Path $toolsBin 'rustc.exe'
$rustfmt = Join-Path $toolsBin 'rustfmt.exe'
$clippy = Join-Path $toolsBin 'clippy-driver.exe'
$scriptPath = Join-Path $repoRoot 'mcw/src/bitcoin_script.rs'
$encodingPath = Join-Path $repoRoot 'mcw/src/bitcoin_encoding.rs'
$testPath = Join-Path $PSScriptRoot 'bitcoin_script_conformance.rs'
$probePath = Join-Path $PSScriptRoot 'bitcoin_script_probe.rs'
$fixtureRoot = Join-Path $PSScriptRoot 'bitcoin_script_fixtures'
$manifest = Get-Content -Raw -LiteralPath (Join-Path $fixtureRoot 'manifest.json') | ConvertFrom-Json
function Hash-Source([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
$hashes = [ordered]@{}
foreach ($path in @($scriptPath, $encodingPath, $testPath, $probePath, (Join-Path $PSScriptRoot 'bitcoin_script_reference.py'))) { $hashes[$path] = Hash-Source $path }
foreach ($name in 'core_vectors.tsv', 'numbers.tsv') {
    $path = Join-Path $fixtureRoot $name
    $hashes[$path] = Hash-Source $path
    if ($hashes[$path] -ne $manifest.hashes.$name.sha256) { throw "Fixture hash mismatch: $name" }
}
$version = & $rustc --version
if ($LASTEXITCODE -ne 0 -or $version -notlike 'rustc 1.99.0 *') { throw 'Existing Rust 1.99.0 toolchain required' }
$buildHandle = $null
try {
    if ((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory -lt 2097152) { Write-Output 'BUILD_DEFERRED_MEMORY'; exit 3 }
    foreach ($number in 1, 2) {
        try {
            $buildHandle = [IO.File]::Open((Join-Path $SharedRoot ".artifacts/mcw-coordination/build-slot-$number.lock"), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
            break
        } catch [IO.IOException] { }
    }
    if ($null -eq $buildHandle) { Write-Output 'BUILD_SLOTS_BUSY'; exit 3 }
    $env:CARGO_BUILD_JOBS = '1'
    & $rustfmt --edition 2024 --check $scriptPath $testPath $probePath
    if ($LASTEXITCODE -ne 0) { throw 'Rust formatting failed' }
    $libHarness = Join-Path $evidenceRoot 'domain.rs'
    $testHarness = Join-Path $evidenceRoot 'tests.rs'
    $probeHarness = Join-Path $evidenceRoot 'probe.rs'
    $domain = 'extern crate self as mcw;' + "`n" + '#[path="' + $encodingPath.Replace('\','/') + '"] pub mod bitcoin_encoding;' + "`n" + '#[path="' + $scriptPath.Replace('\','/') + '"] pub mod bitcoin_script;' + "`n"
    [IO.File]::WriteAllText($libHarness, $domain)
    [IO.File]::WriteAllText($testHarness, $domain + '#[path="' + $testPath.Replace('\','/') + '"] mod conformance;' + "`n")
    [IO.File]::WriteAllText($probeHarness, $domain + '#[path="' + $probePath.Replace('\','/') + '"] mod probe; fn main() { probe::run(); }' + "`n")
    & $clippy --edition=2024 --crate-type=lib --emit=metadata -D warnings $libHarness -o (Join-Path $evidenceRoot 'clippy-lib.rmeta')
    if ($LASTEXITCODE -ne 0) { throw 'Domain Clippy failed' }
    & $clippy --edition=2024 --test --emit=metadata -D warnings $testHarness -o (Join-Path $evidenceRoot 'clippy-tests.rmeta')
    if ($LASTEXITCODE -ne 0) { throw 'Conformance Clippy failed' }
    & $clippy --edition=2024 --emit=metadata -D warnings $probeHarness -o (Join-Path $evidenceRoot 'clippy-probe.rmeta')
    if ($LASTEXITCODE -ne 0) { throw 'Differential probe Clippy failed' }
    $linker = Get-ChildItem -Path 'C:\Program Files\Microsoft Visual Studio\*\*\VC\Tools\MSVC\*\bin\Hostx64\x64\link.exe' | Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $linker) { throw 'Existing MSVC linker required for test harness' }
    $msvcRoot = [IO.Path]::GetFullPath((Join-Path $linker.Directory.FullName '../../..'))
    $sdk = Get-ChildItem -LiteralPath 'C:\Program Files (x86)\Windows Kits\10\Lib' -Directory | Sort-Object Name -Descending | Select-Object -First 1
    $env:PATH = $linker.Directory.FullName + ';' + $env:PATH
    $env:LIB = (Join-Path $msvcRoot 'lib/onecore/x64') + ';' + (Join-Path $sdk.FullName 'ucrt/x64') + ';' + (Join-Path $sdk.FullName 'um/x64')
    $runs = @()
    foreach ($mode in 'debug', 'optimized') {
        $binary = Join-Path $evidenceRoot "script-$mode.exe"
        $arguments = @('--edition=2024', '--test', '-D', 'warnings', '-C', 'overflow-checks=yes', '-C', 'target-feature=+crt-static', $testHarness, '-o', $binary)
        if ($mode -eq 'optimized') { $arguments += @('-C', 'opt-level=2') }
        & $rustc @arguments
        if ($LASTEXITCODE -ne 0) { throw "$mode test build failed" }
        $log = Join-Path $evidenceRoot "$mode-tests.txt"
        & $binary --test-threads=1 2>&1 | Tee-Object -FilePath $log
        if ($LASTEXITCODE -ne 0) { throw "$mode conformance failed" }
        if ((Get-Content -Raw -LiteralPath $log) -notmatch 'test result: ok\. (\d+) passed; 0 failed;') { throw 'Test completion unverified' }
        $runs += [ordered]@{ mode=$mode; passed=[int]$Matches[1]; log=$log }
    }
    $probeBinary = Join-Path $evidenceRoot 'script-probe.exe'
    & $rustc --edition=2024 -D warnings -C overflow-checks=yes -C opt-level=2 -C target-feature=+crt-static $probeHarness -o $probeBinary
    if ($LASTEXITCODE -ne 0) { throw 'Probe compilation failed' }
    if (-not $SkipDifferential) {
        & python (Join-Path $PSScriptRoot 'bitcoin_script_reference.py') --probe $probeBinary --output (Join-Path $evidenceRoot 'differential.json')
        if ($LASTEXITCODE -ne 0) { throw 'Independent differential failed' }
    }
    $sysroot = & $rustc --print sysroot
    $targets = @()
    foreach ($target in 'x86_64-pc-windows-msvc','x86_64-unknown-linux-gnu','aarch64-unknown-linux-gnu','x86_64-apple-darwin','aarch64-apple-darwin') {
        if (-not (Test-Path -LiteralPath (Join-Path $sysroot "lib/rustlib/$target/lib"))) { $targets += @{target=$target; state='unavailable'; reason='target standard library not installed'}; continue }
        & $rustc --edition=2024 --crate-type=lib --emit=metadata --target $target -D warnings $libHarness -o (Join-Path $evidenceRoot "$target.rmeta")
        if ($LASTEXITCODE -ne 0) { throw "$target metadata check failed" }
        $targets += @{target=$target; state='metadata_pass'; reason='runtime/linking not validated by this check'}
    }
    foreach ($path in $hashes.Keys) { if ((Hash-Source $path) -ne $hashes[$path]) { throw "Source changed during verification: $path" } }
    $evidence = [ordered]@{ compiler=$version; edition=2024; sources=$hashes; core_format_vectors=$manifest.core_format_vectors; number_vectors=$manifest.number_vectors; runs=$runs; targets=$targets; production_release=$false; note='Actual source rustc harness. Static CRT is test tooling only, not a shipping mcw build. Host registration, caller migration and five-target release acceptance remain pending.' }
    $evidence | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $evidenceRoot 'verification.json') -Encoding utf8NoBOM
    $evidence | ConvertTo-Json -Depth 8
} finally { if ($null -ne $buildHandle) { $buildHandle.Dispose() } }
