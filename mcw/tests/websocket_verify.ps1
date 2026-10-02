param(
    [string]$SharedRoot = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',
    [string]$RustBin = '',
    [string]$Python = 'C:\Python314\python.exe',
    [switch]$SkipOracle
)
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'Use the equivalent rustc/Python commands in the handoff on other operating systems.' }
$componentRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if (-not $RustBin) { $RustBin = Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin' }
$rustc = Join-Path $RustBin 'rustc.exe'
$rustfmt = Join-Path $RustBin 'rustfmt.exe'
$clippy = Join-Path $RustBin 'clippy-driver.exe'
$rustVersion = (& $rustc --version) -join ''
if ($LASTEXITCODE -or $rustVersion -notmatch '^rustc 1\.99\.0 ') { throw 'Rust 1.99.0 is required; verifier does not install tools.' }
$freeGiB = [double](Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB
if ($freeGiB -lt 2) { throw 'Verification deferred: fewer than 2 GiB free.' }
$slotHandle = $null
foreach ($slot in 1..2) {
    try {
        $slotPath = Join-Path $SharedRoot ".artifacts/mcw-coordination/build-slot-$slot.lock"
        $slotHandle = [IO.File]::Open($slotPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        break
    } catch [IO.IOException] { }
}
if (-not $slotHandle) { throw 'Verification deferred: both shared build slots are occupied.' }
$previousJobs = $env:CARGO_BUILD_JOBS
$env:CARGO_BUILD_JOBS = '1'
try {
    Write-Output ('Build slot ' + $slot + '; free memory GiB ' + $freeGiB.ToString('F2'))
    if (-not $env:VCToolsInstallDir) {
        $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
        $vsRoot = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if (-not $vsRoot) { throw 'Installed native linker unavailable.' }
        & (Join-Path $vsRoot 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
    }
    $output = Join-Path $componentRoot '.artifacts/websocket-verification'
    [IO.Directory]::CreateDirectory($output) | Out-Null
    $started = [DateTime]::UtcNow
    $source = Join-Path $componentRoot 'mcw/src/websocket.rs'
    $testSource = Join-Path $PSScriptRoot 'websocket_conformance.rs'
    $oracleSource = Join-Path $PSScriptRoot 'websocket_oracle.rs'
    $formatFiles = @($source, $testSource)
    if (-not $SkipOracle) { $formatFiles += $oracleSource }
    & $rustfmt --edition 2024 --check @formatFiles
    if ($LASTEXITCODE) { throw 'Formatting check failed.' }
    $harness = Join-Path $output 'lib.rs'
    [IO.File]::WriteAllText($harness, '#[path="../../mcw/src/websocket.rs"] pub mod websocket;' + "`n")
    & $rustc --edition=2024 --crate-name websocket_component --crate-type=lib --emit=metadata -Dwarnings $harness -o (Join-Path $output 'websocket.rmeta')
    if ($LASTEXITCODE) { throw 'Actual-module warning check failed.' }
    & $clippy --edition=2024 --crate-name websocket_component --crate-type=lib --emit=metadata -Dwarnings -Dclippy::all $harness -o (Join-Path $output 'websocket-clippy.rmeta')
    if ($LASTEXITCODE) { throw 'Actual-module Clippy check failed.' }
    foreach ($profile in @('debug', 'optimized')) {
        $binary = Join-Path $output "websocket-$profile.exe"
        $profileArgs = @('-Ccodegen-units=1')
        if ($profile -eq 'optimized') { $profileArgs += '-Copt-level=2' }
        & $rustc --edition=2024 --crate-name websocket_conformance --test -Dwarnings @profileArgs $testSource -o $binary
        if ($LASTEXITCODE) { throw "$profile test compilation failed." }
        & $binary --test-threads=1 2>&1 | Tee-Object -FilePath (Join-Path $output "$profile-results.txt")
        if ($LASTEXITCODE) { throw "$profile conformance tests failed." }
    }
    if (-not $SkipOracle) {
        $oracleBinary = Join-Path $output 'websocket-oracle.exe'
        & $rustc --edition=2024 --crate-name websocket_oracle -Dwarnings -Ccodegen-units=1 $oracleSource -o $oracleBinary
        if ($LASTEXITCODE) { throw 'Actual-module differential harness compilation failed.' }
        & $Python (Join-Path $PSScriptRoot 'websocket_reference.py') --binary $oracleBinary --output $output 2>&1 | Tee-Object -FilePath (Join-Path $output 'oracle-results.txt')
        if ($LASTEXITCODE) { throw 'Independent oracle comparison failed.' }
    }
    $files = @('mcw/src/websocket.rs', 'mcw/tests/websocket_conformance.rs', 'mcw/tests/websocket_verify.ps1')
    if (-not $SkipOracle) { $files += @('mcw/tests/websocket_oracle.rs', 'mcw/tests/websocket_reference.py') }
    $hashes = [ordered]@{}
    foreach ($file in $files) { $hashes[$file] = (Get-FileHash -LiteralPath (Join-Path $componentRoot $file) -Algorithm SHA256).Hash.ToLowerInvariant() }
    [ordered]@{
        scope = 'actual WebSocket component only; synthetic data; no production or five-target claim'
        rust = $rustVersion
        edition = 2024
        native_target = 'x86_64-pc-windows-msvc'
        started_utc = $started.ToString('O')
        completed_utc = [DateTime]::UtcNow.ToString('O')
        format = 'passed'
        warnings = 'denied; passed'
        clippy = 'all denied; passed'
        profiles = @('debug', 'optimized')
        independent_oracle = if ($SkipOracle) { 'not run' } else { 'passed; see oracle.json' }
        source_sha256 = $hashes
        external_crates = 0
        additional_shipping_executables = 0
        remaining = 'native networking/TLS/entropy, production callers, registration, packaging, five-target acceptance'
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $output 'verification.json') -Encoding utf8
    Write-Output ('Evidence: ' + (Join-Path $output 'verification.json'))
} finally {
    $env:CARGO_BUILD_JOBS = $previousJobs
    $slotHandle.Dispose()
}
