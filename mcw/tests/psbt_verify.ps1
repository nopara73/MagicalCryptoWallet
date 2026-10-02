param(
    [string]$RustBin,
    [string]$BitcoinWireSource,
    [string]$BitcoinEncodingSource,
    [switch]$Optimized
)

# Isolated test tooling, never an application package or shipping executable.
$ErrorActionPreference = 'Stop'
$psbtRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$psbtCommonDirectory = & git -C $psbtRoot rev-parse --path-format=absolute --git-common-dir
if ($LASTEXITCODE -ne 0) { throw 'Cannot locate repository coordination root.' }
$psbtSharedRoot = Split-Path -Parent ($psbtCommonDirectory | Select-Object -First 1)
if (-not $RustBin) {
    $RustBin = Join-Path $psbtSharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin'
}
if (-not $BitcoinWireSource) { $BitcoinWireSource = Join-Path $psbtRoot 'mcw/src/bitcoin_wire.rs' }
if (-not $BitcoinEncodingSource) { $BitcoinEncodingSource = Join-Path $psbtRoot 'mcw/src/bitcoin_encoding.rs' }
$psbtExecutableSuffix = if ($IsWindows) { '.exe' } else { '' }
$psbtCompiler = Join-Path $RustBin "rustc$psbtExecutableSuffix"
$psbtFormatter = Join-Path $RustBin "rustfmt$psbtExecutableSuffix"
$psbtClippy = Join-Path $RustBin "clippy-driver$psbtExecutableSuffix"
foreach ($psbtRequired in @($psbtCompiler, $psbtFormatter, $psbtClippy, $BitcoinWireSource, $BitcoinEncodingSource)) {
    if (-not (Test-Path -LiteralPath $psbtRequired -PathType Leaf)) { throw "Missing verification input: $psbtRequired" }
}
$psbtCompilerVersion = & $psbtCompiler --version
if ($LASTEXITCODE -ne 0 -or $psbtCompilerVersion -notmatch '^rustc 1\.99\.0 ') { throw 'Rust 1.99.0 is required.' }

# Snapshot actual first-party source to avoid compiling files a peer is editing.
# No replacement stubs, downloaded crates, mock codecs, or Cargo package are used.
$psbtRunId = (Get-Date -Format 'yyyyMMdd-HHmmss-fff') + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8)
$psbtOutput = Join-Path $psbtRoot ".artifacts/psbt-validation/$psbtRunId"
$psbtSnapshot = Join-Path $psbtOutput 'snapshot'
New-Item -ItemType Directory -Path $psbtSnapshot -Force | Out-Null
$psbtInputs = [ordered]@{
    'psbt.rs' = Join-Path $psbtRoot 'mcw/src/psbt.rs'
    'bitcoin_wire.rs' = $BitcoinWireSource
    'bitcoin_encoding.rs' = $BitcoinEncodingSource
    'psbt_conformance.rs' = Join-Path $PSScriptRoot 'psbt_conformance.rs'
    'psbt_vectors.tsv' = Join-Path $PSScriptRoot 'psbt_vectors.tsv'
    'psbt_managed_vectors.tsv' = Join-Path $PSScriptRoot 'psbt_managed_vectors.tsv'
}
$psbtInputEvidence = foreach ($psbtEntry in $psbtInputs.GetEnumerator()) {
    $psbtDestination = Join-Path $psbtSnapshot $psbtEntry.Key
    Copy-Item -LiteralPath $psbtEntry.Value -Destination $psbtDestination
    [ordered]@{ file = $psbtEntry.Key; source = [IO.Path]::GetFullPath($psbtEntry.Value); sha256 = (Get-FileHash -LiteralPath $psbtDestination -Algorithm SHA256).Hash.ToLowerInvariant() }
}
$psbtHarness = Join-Path $psbtOutput 'harness.rs'
$psbtHarnessText = @'
extern crate self as mcw;
#[path = "snapshot/bitcoin_encoding.rs"] pub mod bitcoin_encoding;
#[path = "snapshot/bitcoin_wire.rs"] pub mod bitcoin_wire;
#[path = "snapshot/psbt.rs"] pub mod psbt;
#[path = "snapshot/psbt_conformance.rs"] mod psbt_conformance;
'@
[IO.File]::WriteAllText($psbtHarness, $psbtHarnessText, [Text.UTF8Encoding]::new($false))

$psbtBuildSlot = $null
$psbtSlotNumber = $null
foreach ($psbtIndex in 1..2) {
    try {
        $psbtBuildSlot = [IO.File]::Open((Join-Path $psbtSharedRoot ".artifacts/mcw-coordination/build-slot-$psbtIndex.lock"), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        $psbtSlotNumber = $psbtIndex
        break
    } catch [IO.IOException] {}
}
if ($null -eq $psbtBuildSlot) { throw 'Both build slots are occupied; preserved snapshot can be reviewed while waiting.' }
try {
    if ($IsWindows) {
        $psbtFreeBytes = (Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory * 1KB
        if ($psbtFreeBytes -lt 2GB) { throw 'Less than 2 GiB RAM is free; defer compilation.' }
        if (-not $env:VCToolsInstallDir) {
            $psbtVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
            $psbtVsRoot = & $psbtVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
            if ($LASTEXITCODE -ne 0 -or -not $psbtVsRoot) { throw 'Existing Visual Studio C++ build tools are required for native tests.' }
            & (Join-Path $psbtVsRoot 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
        }
    }
    & $psbtFormatter --edition 2024 --check $psbtInputs['psbt.rs'] $psbtInputs['psbt_conformance.rs']
    if ($LASTEXITCODE -ne 0) { throw 'PSBT formatting check failed.' }
    & $psbtClippy --edition=2024 --test --emit=metadata -D warnings $psbtHarness -o (Join-Path $psbtOutput 'clippy.rmeta') 2>&1 | Tee-Object -FilePath (Join-Path $psbtOutput 'clippy.log')
    if ($LASTEXITCODE -ne 0) { throw 'PSBT Clippy check failed.' }
    $psbtTestBinary = Join-Path $psbtOutput "psbt-conformance$psbtExecutableSuffix"
    $psbtArguments = @('--edition=2024', '--test', '-C', 'codegen-units=1', '-C', 'overflow-checks=yes', '-D', 'warnings', $psbtHarness, '-o', $psbtTestBinary)
    if ($Optimized) { $psbtArguments += '-O' }
    & $psbtCompiler @psbtArguments 2>&1 | Tee-Object -FilePath (Join-Path $psbtOutput 'compile.log')
    if ($LASTEXITCODE -ne 0) { throw 'PSBT compilation failed.' }
    & $psbtTestBinary --test-threads=1 2>&1 | Tee-Object -FilePath (Join-Path $psbtOutput 'tests.log')
    if ($LASTEXITCODE -ne 0) { throw 'PSBT conformance tests failed.' }
    $psbtEvidence = [ordered]@{
        task_slug = 'psbt'
        compiler = $psbtCompilerVersion
        target = if ($IsWindows) { 'x86_64-pc-windows-msvc' } else { 'native host' }
        optimized = [bool]$Optimized
        overflow_checks = $true
        format = 'passed'
        clippy = 'passed with warnings denied'
        tests = 'passed'
        build_slot = $psbtSlotNumber
        inputs = @($psbtInputEvidence)
        product_release = $false
    }
    $psbtEvidence | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $psbtOutput 'evidence.json') -Encoding utf8
    Write-Output "PSBT_EVIDENCE=$psbtOutput"
} finally {
    $psbtBuildSlot.Dispose()
}
