param(
    [string]$SharedRoot = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',
    [string]$ReferenceDirectory = '',
    [string]$Python = 'C:\Python314\python.exe'
)
$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$evidenceRoot = Join-Path $repoRoot '.artifacts/bitcoin-block-evidence'
$harnessRoot = Join-Path $evidenceRoot 'harness'
$toolBin = Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin'
$rustc = Join-Path $toolBin 'rustc.exe'
New-Item -ItemType Directory -Force -Path $harnessRoot | Out-Null
function Source-Hash([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Text-Hash([string]$Path) {
    $canonical = [IO.File]::ReadAllText($Path).Replace("`r`n", "`n")
    [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($canonical))).ToLowerInvariant()
}
$sources = [ordered]@{}
foreach ($name in 'bitcoin_encoding', 'bitcoin_wire', 'bitcoin_block') {
    $sources[$name] = Join-Path $repoRoot ('mcw/src/' + $name + '.rs')
    if (-not (Test-Path -LiteralPath $sources[$name])) { throw ('Actual source missing: ' + $sources[$name]) }
}
$sourceHashes = [ordered]@{}
foreach ($name in $sources.Keys) { $sourceHashes[$name] = Source-Hash $sources[$name] }
$testSource = Join-Path $PSScriptRoot 'bitcoin_block_conformance.rs'
$driverSource = Join-Path $PSScriptRoot 'bitcoin_block_fixtures/driver.rs'
$sourceHashes['conformance'] = Source-Hash $testSource
$sourceHashes['driver'] = Source-Hash $driverSource
$manifestPath = Join-Path $PSScriptRoot 'bitcoin_block_fixtures/manifest.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
foreach ($file in $manifest.files.PSObject.Properties) {
    if ((Text-Hash (Join-Path $PSScriptRoot ('bitcoin_block_fixtures/' + $file.Name))) -ne $file.Value) { throw ('Fixture hash mismatch: ' + $file.Name) }
}
$buildHandle = $null
try {
    if ((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory -lt 2097152) { Write-Output 'Build deferred: less than 2 GiB free memory'; exit 75 }
    foreach ($slot in 1, 2) {
        try { $buildHandle = [IO.File]::Open((Join-Path $SharedRoot ('.artifacts/mcw-coordination/build-slot-' + $slot + '.lock')), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None); break } catch [IO.IOException] { }
    }
    if (-not $buildHandle) { Write-Output 'Build deferred: both compiler slots occupied'; exit 75 }
    $version = & $rustc --version
    if ($LASTEXITCODE -ne 0 -or $version -notlike 'rustc 1.99.0 *') { throw 'Rust 1.99.0 required' }
    $moduleText = 'extern crate self as mcw;' + [Environment]::NewLine
    foreach ($name in $sources.Keys) { $moduleText += '#[path="' + $sources[$name].Replace('\', '/') + '"] pub mod ' + $name + ';' + [Environment]::NewLine }
    $librarySource = Join-Path $harnessRoot 'library.rs'
    $harnessSource = Join-Path $harnessRoot 'conformance.rs'
    $referenceSource = Join-Path $harnessRoot 'reference.rs'
    [IO.File]::WriteAllText($librarySource, $moduleText)
    [IO.File]::WriteAllText($harnessSource, $moduleText + '#[path="' + $testSource.Replace('\', '/') + '"] mod conformance;')
    [IO.File]::WriteAllText($referenceSource, $moduleText + '#[path="' + $driverSource.Replace('\', '/') + '"] mod reference_driver;' + [Environment]::NewLine + 'fn main() { reference_driver::main(); }')
    $linker = Get-ChildItem -Path 'C:\Program Files\Microsoft Visual Studio\*\*\VC\Tools\MSVC\*\bin\Hostx64\x64\link.exe' | Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $linker) { throw 'Existing MSVC linker is required' }
    $msvcRoot = [IO.Path]::GetFullPath((Join-Path $linker.Directory.FullName '../../..'))
    $sdk = Get-ChildItem -LiteralPath 'C:\Program Files (x86)\Windows Kits\10\Lib' -Directory | Sort-Object Name -Descending | Select-Object -First 1
    $env:PATH = $linker.Directory.FullName + ';' + $env:PATH
    $env:LIB = (Join-Path $msvcRoot 'lib/onecore/x64') + ';' + (Join-Path $sdk.FullName 'ucrt/x64') + ';' + (Join-Path $sdk.FullName 'um/x64')
    $env:CARGO_BUILD_JOBS = '1'
    & (Join-Path $toolBin 'rustfmt.exe') --edition 2024 --check $sources.bitcoin_block $testSource $driverSource
    if ($LASTEXITCODE -ne 0) { throw 'rustfmt check failed' }
    $common = @('--edition=2024', '-D', 'warnings', '-C', 'overflow-checks=yes', '-C', 'codegen-units=1', '-C', 'target-feature=+crt-static')
    & $rustc @common --crate-type lib --emit metadata $librarySource -o (Join-Path $harnessRoot 'block.rmeta')
    if ($LASTEXITCODE -ne 0) { throw 'Actual-source library metadata failed' }
    & (Join-Path $toolBin 'clippy-driver.exe') @common --crate-type lib --emit metadata $librarySource -o (Join-Path $harnessRoot 'clippy.rmeta')
    if ($LASTEXITCODE -ne 0) { throw 'Actual-source Clippy failed' }
    $runs = @()
    foreach ($mode in 'debug', 'optimized') {
        $binary = Join-Path $harnessRoot ('block-' + $mode + '.exe')
        $arguments = $common + @('--test', $harnessSource, '-o', $binary)
        if ($mode -eq 'optimized') { $arguments += @('-C', 'opt-level=2') }
        & $rustc @arguments
        if ($LASTEXITCODE -ne 0) { throw ($mode + ' conformance compilation failed') }
        $log = Join-Path $evidenceRoot ($mode + '.log')
        & $binary --test-threads=1 2>&1 | Tee-Object -FilePath $log
        if ($LASTEXITCODE -ne 0) { throw ($mode + ' tests failed') }
        $output = Get-Content -LiteralPath $log -Raw
        if ($output -notmatch 'test result: ok\. (\d+) passed; 0 failed;') { throw 'Test completion not verified' }
        $runs += [ordered]@{mode=$mode;passed=[int]$Matches[1];failed=0;log=$log;binary_sha256=(Source-Hash $binary)}
    }
    $driver = Join-Path $harnessRoot 'reference-driver.exe'
    & $rustc @common -C opt-level=2 $referenceSource -o $driver
    if ($LASTEXITCODE -ne 0) { throw 'Actual-source reference driver compilation failed' }
    $differential = 'not run: provide hash-pinned ReferenceDirectory'
    if ($ReferenceDirectory) {
        & $Python (Join-Path $PSScriptRoot 'bitcoin_block_reference.py') differential --references $ReferenceDirectory --driver $driver --evidence $evidenceRoot
        if ($LASTEXITCODE -ne 0) { throw 'Independent reference checks failed' }
        $differential = Join-Path $evidenceRoot 'differential.json'
    }
    $dumpbin = Join-Path $linker.Directory.FullName 'dumpbin.exe'
    $imports = Join-Path $evidenceRoot 'runtime-imports.txt'
    & $dumpbin /dependents (Join-Path $harnessRoot 'block-optimized.exe') | Set-Content -LiteralPath $imports -Encoding utf8
    if ($LASTEXITCODE -ne 0) { throw 'Runtime import audit failed' }
    foreach ($name in $sources.Keys) { if ((Source-Hash $sources[$name]) -ne $sourceHashes[$name]) { throw 'Actual source changed during verification' } }
    if ((Source-Hash $testSource) -ne $sourceHashes.conformance -or (Source-Hash $driverSource) -ne $sourceHashes.driver) { throw 'Test source changed during verification' }
    $platforms = @()
    foreach ($target in 'x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu', 'x86_64-apple-darwin', 'aarch64-apple-darwin') {
        $available = Test-Path -LiteralPath (Join-Path $toolBin ('../lib/rustlib/' + $target + '/lib'))
        $platforms += [ordered]@{target=$target;standard_library_available=$available;execution_verified=($target -eq 'x86_64-pc-windows-msvc');note= $(if($target -eq 'x86_64-pc-windows-msvc') {'actual debug/optimized test execution'} else {'native compilation and execution not verified'})}
    }
    $evidence = [ordered]@{compiler=$version;edition=2024;sources=$sourceHashes;source_paths=$sources;runs=$runs;block_vectors=$manifest.block_vectors;partial_vectors=$manifest.partial_vectors;witness_transactions=$manifest.witness_transactions;differential=$differential;runtime_imports=$imports;platforms=$platforms;production_release=$false;note='Actual committed-module sources in ignored test harness only. No extra Cargo package or shipping executable.'}
    $evidence | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $evidenceRoot 'verification.json') -Encoding utf8
    $evidence | ConvertTo-Json -Depth 8
} finally { if ($buildHandle) { $buildHandle.Dispose() } }
