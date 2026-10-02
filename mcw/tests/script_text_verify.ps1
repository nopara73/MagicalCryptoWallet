param(
    [string]$SharedRoot = 'C:/Users/user/OneDrive/Documents/ChatGPT/MagicalCryptoWallet',
    [switch]$PrepareFixtures,
    [switch]$SkipDifferential
)
$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$evidenceRoot = Join-Path $repoRoot '.artifacts/script-text-evidence'
New-Item -ItemType Directory -Force -Path $evidenceRoot | Out-Null
$toolsBin = Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin'
$rustc = Join-Path $toolsBin 'rustc.exe'
$rustfmt = Join-Path $toolsBin 'rustfmt.exe'
$clippy = Join-Path $toolsBin 'clippy-driver.exe'
$python = 'C:/Python314/python.exe'
$textPath = Join-Path $repoRoot 'mcw/src/script_text.rs'
$scriptPath = Join-Path $repoRoot 'mcw/src/bitcoin_script.rs'
$encodingPath = Join-Path $repoRoot 'mcw/src/bitcoin_encoding.rs'
$testPath = Join-Path $PSScriptRoot 'script_text_conformance.rs'
$probePath = Join-Path $PSScriptRoot 'script_text_probe.rs'
$referencePath = Join-Path $PSScriptRoot 'script_text_reference.py'
$fixtureRoot = Join-Path $PSScriptRoot 'script_text_fixtures'
$referenceRoot = Join-Path $PSScriptRoot 'script_text_reference'
$assembly = 'C:/Users/user/.nuget/packages/nbitcoin/10.0.13/lib/net10.0/NBitcoin.dll'
function Hash-File([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
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
    & $rustfmt --edition 2024 --check $textPath $testPath $probePath
    if ($LASTEXITCODE -ne 0) { throw 'Rust formatting failed' }
    $oracle = Join-Path $evidenceRoot 'oracle/bin/Debug/net10.0/Oracle.dll'
    if ($PrepareFixtures -or -not $SkipDifferential) {
        if (-not (Test-Path -LiteralPath $assembly)) { throw 'Exact retained NBitcoin10.0.13 cached development oracle unavailable' }
        $emptyFeed = Join-Path $evidenceRoot 'empty-feed'
        New-Item -ItemType Directory -Path $emptyFeed -Force | Out-Null
        & dotnet build (Join-Path $referenceRoot 'Oracle.csproj') --source $emptyFeed --disable-build-servers --nologo -m:1 -p:UseSharedCompilation=false -p:ImportDirectoryBuildProps=false -p:ImportDirectoryBuildTargets=false -p:NuGetPackageRoot=C:/Users/user/.nuget/packages/ "-p:BaseOutputPath=$evidenceRoot/oracle/bin/" "-p:BaseIntermediateOutputPath=$evidenceRoot/oracle/obj/" -v:minimal
        if ($LASTEXITCODE -ne 0) { throw 'Development-only retained reference build failed' }
    }
    if ($PrepareFixtures) {
        & $python $referencePath --prepare --oracle $oracle --assembly $assembly
        if ($LASTEXITCODE -ne 0) { throw 'Reference fixture preparation failed' }
    }
    $manifest = Get-Content -Raw -LiteralPath (Join-Path $fixtureRoot 'manifest.json') | ConvertFrom-Json
    $fixturePath = Join-Path $fixtureRoot 'vectors.tsv'
    if ((Hash-File $fixturePath) -ne $manifest.fixtures_sha256) { throw 'Reference fixture hash mismatch' }
    if ((Hash-File (Join-Path $referenceRoot 'Program.cs')) -ne $manifest.oracle_program_sha256) { throw 'Reference program provenance mismatch' }
    if ((Hash-File $referencePath) -ne $manifest.generator_sha256) { throw 'Reference generator provenance mismatch' }
    $hashes = [ordered]@{}
    foreach ($path in @($textPath, $scriptPath, $encodingPath, $testPath, $probePath, $referencePath, $fixturePath, (Join-Path $referenceRoot 'Program.cs'))) { $hashes[$path] = Hash-File $path }
    $domain = 'extern crate self as mcw;' + "`n" + '#[path="' + $encodingPath.Replace('\','/') + '"] pub mod bitcoin_encoding;' + "`n" + '#[path="' + $scriptPath.Replace('\','/') + '"] pub mod bitcoin_script;' + "`n" + '#[path="' + $textPath.Replace('\','/') + '"] pub mod script_text;' + "`n"
    $libHarness = Join-Path $evidenceRoot 'domain.rs'
    $testHarness = Join-Path $evidenceRoot 'tests.rs'
    $probeHarness = Join-Path $evidenceRoot 'probe.rs'
    [IO.File]::WriteAllText($libHarness, $domain)
    [IO.File]::WriteAllText($testHarness, $domain + '#[path="' + $testPath.Replace('\','/') + '"] mod conformance;' + "`n")
    [IO.File]::WriteAllText($probeHarness, $domain + '#[path="' + $probePath.Replace('\','/') + '"] mod probe; fn main() { probe::run(); }' + "`n")
    & $clippy --edition=2024 --crate-type=lib --emit=metadata -D warnings $libHarness -o (Join-Path $evidenceRoot 'clippy-lib.rmeta')
    if ($LASTEXITCODE -ne 0) { throw 'Domain Clippy failed' }
    & $clippy --edition=2024 --test --emit=metadata -D warnings $testHarness -o (Join-Path $evidenceRoot 'clippy-tests.rmeta')
    if ($LASTEXITCODE -ne 0) { throw 'Conformance Clippy failed' }
    & $clippy --edition=2024 --emit=metadata -D warnings $probeHarness -o (Join-Path $evidenceRoot 'clippy-probe.rmeta')
    if ($LASTEXITCODE -ne 0) { throw 'Probe Clippy failed' }
    $linker = Get-ChildItem -Path 'C:/Program Files/Microsoft Visual Studio/*/*/VC/Tools/MSVC/*/bin/Hostx64/x64/link.exe' | Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $linker) { throw 'Existing MSVC linker required for test tooling' }
    $msvcRoot = [IO.Path]::GetFullPath((Join-Path $linker.Directory.FullName '../../..'))
    $sdk = Get-ChildItem -LiteralPath 'C:/Program Files (x86)/Windows Kits/10/Lib' -Directory | Sort-Object Name -Descending | Select-Object -First 1
    $env:PATH = $linker.Directory.FullName + ';' + $env:PATH
    $env:LIB = (Join-Path $msvcRoot 'lib/onecore/x64') + ';' + (Join-Path $sdk.FullName 'ucrt/x64') + ';' + (Join-Path $sdk.FullName 'um/x64')
    $runs = @()
    foreach ($mode in 'debug', 'optimized') {
        $binary = Join-Path $evidenceRoot "text-$mode.exe"
        $arguments = @('--edition=2024', '--test', '-D', 'warnings', '-C', 'overflow-checks=yes', '-C', 'target-feature=+crt-static', $testHarness, '-o', $binary)
        if ($mode -eq 'optimized') { $arguments += @('-C', 'opt-level=2') }
        & $rustc @arguments
        if ($LASTEXITCODE -ne 0) { throw "$mode test build failed" }
        $log = Join-Path $evidenceRoot "$mode-tests.txt"
        & $binary --test-threads=1 2>&1 | Tee-Object -FilePath $log
        if ($LASTEXITCODE -ne 0) { throw "$mode conformance failed" }
        if ((Get-Content -Raw -LiteralPath $log) -notmatch 'test result: ok\. (\d+) passed; 0 failed;') { throw 'Test completion unverified' }
        $runs += [ordered]@{mode=$mode;passed=[int]$Matches[1];log=$log}
    }
    $probe = Join-Path $evidenceRoot 'text-probe.exe'
    & $rustc --edition=2024 -D warnings -C overflow-checks=yes -C opt-level=2 -C target-feature=+crt-static $probeHarness -o $probe
    if ($LASTEXITCODE -ne 0) { throw 'Probe compilation failed' }
    if (-not $SkipDifferential) {
        if ((Hash-File $assembly) -ne $manifest.oracle_assembly_sha256) { throw 'Actual retained assembly differs from fixture provenance' }
        & $python $referencePath --oracle $oracle --probe $probe --output (Join-Path $evidenceRoot 'differential.json')
        if ($LASTEXITCODE -ne 0) { throw 'Independent retained-library comparison failed' }
    }
    foreach ($path in $hashes.Keys) { if ((Hash-File $path) -ne $hashes[$path]) { throw "Source changed during verification: $path" } }
    $evidence = [ordered]@{compiler=$version;edition=2024;sources=$hashes;reference_cases=$manifest.cases;parse_cases=$manifest.parse_cases;render_cases=$manifest.render_cases;runs=$runs;production_release=$false;note='Actual first-party portable source; retained NBitcoin assembly is an ignored development oracle only. Static CRT is test tooling only. Shared native caller incorporation and other platform/runtime acceptance remain separate.'}
    [IO.File]::WriteAllText((Join-Path $evidenceRoot 'verification.json'), ($evidence | ConvertTo-Json -Depth 8) + "`n", [Text.UTF8Encoding]::new($false))
    $evidence | ConvertTo-Json -Depth 8
} finally { if ($null -ne $buildHandle) { $buildHandle.Dispose() } }
