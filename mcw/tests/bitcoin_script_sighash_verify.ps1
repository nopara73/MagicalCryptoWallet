param([string]$SharedRoot = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet')
$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$evidenceRoot = Join-Path $repoRoot '.artifacts/bitcoin-script-evidence/sighash'
New-Item -ItemType Directory -Force -Path $evidenceRoot | Out-Null
$toolsBin = Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin'
$rustc = Join-Path $toolsBin 'rustc.exe'; $fmt = Join-Path $toolsBin 'rustfmt.exe'; $clippy = Join-Path $toolsBin 'clippy-driver.exe'
$version = & $rustc --version
if ($LASTEXITCODE -ne 0 -or $version -notlike 'rustc 1.99.0 *') { throw 'Rust 1.99.0 required' }
$modules = [ordered]@{bitcoin_encoding='mcw/src/bitcoin_encoding.rs';bitcoin_wire='mcw/src/bitcoin_wire.rs';bitcoin_script='mcw/src/bitcoin_script.rs';script_service='mcw/src/script_service/mod.rs'}
$testPath = Join-Path $PSScriptRoot 'bitcoin_script_sighash_conformance.rs'
$probePath = Join-Path $PSScriptRoot 'bitcoin_script_sighash_probe.rs'
$hashes = [ordered]@{}
foreach ($path in @($modules.Values | ForEach-Object { Join-Path $repoRoot $_ }) + @($testPath,$probePath,(Join-Path $repoRoot 'mcw/src/script_service/sighash.rs'),(Join-Path $PSScriptRoot 'bitcoin_script_sighash_reference.py'))) { $hashes[$path] = (Get-FileHash -LiteralPath $path).Hash.ToLowerInvariant() }
$manifest = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'bitcoin_script_fixtures/sighash_manifest.json') | ConvertFrom-Json
foreach ($name in 'legacy_sighash.tsv','segwit_sighash.tsv','taproot_sighash.tsv') {
    $path = Join-Path $PSScriptRoot "bitcoin_script_fixtures/$name"
    $hashes[$path] = (Get-FileHash -LiteralPath $path).Hash.ToLowerInvariant()
    if ($hashes[$path] -ne $manifest.hashes.$name) { throw "Sighash fixture hash mismatch: $name" }
}
$handle = $null
try {
    if ((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory -lt 2097152) { Write-Output 'BUILD_DEFERRED_MEMORY'; exit 3 }
    foreach ($number in 1,2) {
        try { $handle = [IO.File]::Open((Join-Path $SharedRoot ".artifacts/mcw-coordination/build-slot-$number.lock"),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None); break } catch [IO.IOException] { }
    }
    if (-not $handle) { Write-Output 'BUILD_SLOTS_BUSY'; exit 3 }
    $env:CARGO_BUILD_JOBS = '1'
    & $fmt --edition 2024 --check (Join-Path $repoRoot 'mcw/src/script_service/mod.rs') (Join-Path $repoRoot 'mcw/src/script_service/sighash.rs') $testPath $probePath
    if ($LASTEXITCODE -ne 0) { throw 'Sighash Rustfmt failed' }
    $domain = 'extern crate self as mcw;' + "`n"
    foreach ($name in $modules.Keys) { $domain += '#[path="' + (Join-Path $repoRoot $modules[$name]).Replace('\','/') + '"] pub mod ' + $name + ';' + "`n" }
    $domainPath = Join-Path $evidenceRoot 'domain.rs'; $harnessPath = Join-Path $evidenceRoot 'tests.rs'; $probeHarness = Join-Path $evidenceRoot 'probe.rs'
    [IO.File]::WriteAllText($domainPath,$domain)
    [IO.File]::WriteAllText($harnessPath,$domain + '#[path="' + $testPath.Replace('\','/') + '"] mod conformance;')
    [IO.File]::WriteAllText($probeHarness,$domain + '#[path="' + $probePath.Replace('\','/') + '"] mod probe; fn main() { probe::run(); }')
    & $clippy --edition=2024 --crate-type=lib --emit=metadata -D warnings $domainPath -o (Join-Path $evidenceRoot 'clippy-domain.rmeta')
    if ($LASTEXITCODE -ne 0) { throw 'Sighash domain Clippy failed' }
    & $clippy --edition=2024 --test --emit=metadata -D warnings $harnessPath -o (Join-Path $evidenceRoot 'clippy-tests.rmeta')
    if ($LASTEXITCODE -ne 0) { throw 'Sighash tests Clippy failed' }
    & $clippy --edition=2024 --emit=metadata -D warnings $probeHarness -o (Join-Path $evidenceRoot 'clippy-probe.rmeta')
    if ($LASTEXITCODE -ne 0) { throw 'Sighash probe Clippy failed' }
    $linker = Get-ChildItem -Path 'C:\Program Files\Microsoft Visual Studio\*\*\VC\Tools\MSVC\*\bin\Hostx64\x64\link.exe' | Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $linker) { throw 'Existing MSVC linker required for test harness' }
    $msvcRoot = [IO.Path]::GetFullPath((Join-Path $linker.Directory.FullName '../../..'))
    $sdk = Get-ChildItem -LiteralPath 'C:\Program Files (x86)\Windows Kits\10\Lib' -Directory | Sort-Object Name -Descending | Select-Object -First 1
    $env:PATH = $linker.Directory.FullName + ';' + $env:PATH
    $env:LIB = (Join-Path $msvcRoot 'lib/onecore/x64') + ';' + (Join-Path $sdk.FullName 'ucrt/x64') + ';' + (Join-Path $sdk.FullName 'um/x64')
    $runs = @()
    foreach ($mode in 'debug','optimized') {
        $binary = Join-Path $evidenceRoot "sighash-$mode.exe"
        $arguments = @('--edition=2024','--test','-D','warnings','-C','overflow-checks=yes','-C','target-feature=+crt-static',$harnessPath,'-o',$binary)
        if ($mode -eq 'optimized') { $arguments += @('-C','opt-level=2') }
        & $rustc @arguments
        if ($LASTEXITCODE -ne 0) { throw "$mode sighash build failed" }
        $log = Join-Path $evidenceRoot "$mode-tests.txt"
        & $binary --test-threads=1 2>&1 | Tee-Object -FilePath $log
        if ($LASTEXITCODE -ne 0) { throw "$mode sighash tests failed" }
        if ((Get-Content -Raw -LiteralPath $log) -notmatch 'test result: ok\. (\d+) passed; 0 failed;') { throw 'Sighash test completion unverified' }
        $runs += @{mode=$mode;passed=[int]$Matches[1];log=$log}
    }
    $probeBinary = Join-Path $evidenceRoot 'sighash-probe.exe'
    & $rustc --edition=2024 -D warnings -C overflow-checks=yes -C opt-level=2 -C target-feature=+crt-static $probeHarness -o $probeBinary
    if ($LASTEXITCODE -ne 0) { throw 'Sighash probe build failed' }
    & python (Join-Path $PSScriptRoot 'bitcoin_script_sighash_reference.py') --probe $probeBinary --output (Join-Path $evidenceRoot 'differential.json')
    if ($LASTEXITCODE -ne 0) { throw 'Sighash differential failed' }
    foreach ($path in $hashes.Keys) { if ((Get-FileHash -LiteralPath $path).Hash.ToLowerInvariant() -ne $hashes[$path]) { throw "Source changed during verification: $path" } }
    $evidence = [ordered]@{compiler=$version;edition=2024;sources=$hashes;runs=$runs;legacy_vectors=$manifest.legacy_vectors;segwit_vectors=$manifest.segwit_vectors;taproot_vectors=$manifest.taproot_vectors;production_release=$false;note='Actual portable source. No interpreter, signature verifier, production bridge/caller or five-target acceptance is claimed from sighash evidence. Static CRT in ignored Windows test tooling only.'}
    $evidence | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $evidenceRoot 'verification.json') -Encoding utf8NoBOM
    $evidence | ConvertTo-Json -Depth 8
} finally { if ($handle) { $handle.Dispose() } }
