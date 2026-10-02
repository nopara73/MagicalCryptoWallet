param(
    [string]$SharedRoot = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',
    [string]$RustBin = '',
    [switch]$SkipClippy
)
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'Use the rustc commands in the handoff for other operating systems.' }
$componentRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if (-not $RustBin) { $RustBin = Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin' }
$rustc = Join-Path $RustBin 'rustc.exe'
$rustfmt = Join-Path $RustBin 'rustfmt.exe'
$clippy = Join-Path $RustBin 'clippy-driver.exe'
$rustVersion = (& $rustc --version) -join ''
if ($LASTEXITCODE -or $rustVersion -notmatch '^rustc 1\.99\.0 ') { throw 'Rust 1.99.0 is required.' }
$availableGiB = [double](Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB
Write-Output ('Free memory GiB: {0:N2}' -f $availableGiB)
if ($availableGiB -lt 2) { throw 'Verification deferred: fewer than 2 GiB free.' }
$slotHandle = $null
$slotPath = $null
foreach ($slot in 1..2) {
    $candidate = Join-Path $SharedRoot ".artifacts/mcw-coordination/build-slot-$slot.lock"
    try {
        $slotHandle = [IO.File]::Open($candidate, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        $slotPath = $candidate
        break
    } catch [IO.IOException] { }
}
if (-not $slotHandle) { throw 'Verification deferred: both shared build slots are occupied.' }
$previousJobs = $env:CARGO_BUILD_JOBS
$env:CARGO_BUILD_JOBS = '1'
try {
    Write-Output ('Build slot: ' + [IO.Path]::GetFileName($slotPath))
    if (-not $env:VCToolsInstallDir) {
        $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
        $vsRoot = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if (-not $vsRoot) { throw 'The installed native linker was not found; this verifier never installs tools.' }
        & (Join-Path $vsRoot 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
    }
    $output = Join-Path $componentRoot '.artifacts/socks5-verification'
    [IO.Directory]::CreateDirectory($output) | Out-Null
    $started = [DateTime]::UtcNow
    $source = Join-Path $componentRoot 'mcw/src/socks5.rs'
    $wireTest = Join-Path $PSScriptRoot 'socks5_wire.rs'
    $transportTest = Join-Path $PSScriptRoot 'socks5_transport.rs'
    & $rustfmt --edition 2024 --check $source $wireTest $transportTest
    if ($LASTEXITCODE) { throw 'Formatting check failed.' }
    $moduleHarness = Join-Path $output 'lib.rs'
    [IO.File]::WriteAllText($moduleHarness, '#[path="../../mcw/src/socks5.rs"] pub mod socks5;' + "`n")
    & $rustc --edition=2024 --crate-name socks5_component --crate-type=lib --emit=metadata -Dwarnings $moduleHarness -o (Join-Path $output 'socks5.rmeta')
    if ($LASTEXITCODE) { throw 'Module warning check failed.' }
    if (-not $SkipClippy) {
        & $clippy --edition=2024 --crate-name socks5_component --crate-type=lib --emit=metadata -Dwarnings -Dclippy::all $moduleHarness -o (Join-Path $output 'socks5-clippy.rmeta')
        if ($LASTEXITCODE) { throw 'Module Clippy check failed.' }
    }
    foreach ($name in @('wire', 'transport')) {
        $testSource = Join-Path $PSScriptRoot "socks5_$name.rs"
        $testBinary = Join-Path $output "socks5_$name.exe"
        & $rustc --edition=2024 --crate-name "socks5_$name" --test -Dwarnings $testSource -o $testBinary
        if ($LASTEXITCODE) { throw "$name test compilation failed." }
        & $testBinary --test-threads=1 2>&1 | Tee-Object -FilePath (Join-Path $output "$name-results.txt")
        if ($LASTEXITCODE) { throw "$name tests failed." }
    }
    $result = [ordered]@{
        scope = 'socks5 component only; synthetic loopback proxies'
        rust = $rustVersion
        edition = 2024
        native_test_target = 'x86_64-pc-windows-msvc'
        started_utc = $started.ToString('O')
        completed_utc = [DateTime]::UtcNow.ToString('O')
        format = 'passed'
        warnings = 'denied; passed'
        clippy = if ($SkipClippy) { 'not run' } else { 'all denied; passed' }
        test_logs = @('wire-results.txt', 'transport-results.txt')
        source_sha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
        shipping_cargo_manifests_modified = $false
        external_crates = 0
        shipping_runtime_audit = 'host integration and five-target packaging still required'
    }
    $result | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $output 'verification.json') -Encoding utf8
    Write-Output ('Verified component: ' + (Join-Path $output 'verification.json'))
} finally {
    $env:CARGO_BUILD_JOBS = $previousJobs
    $slotHandle.Dispose()
}
