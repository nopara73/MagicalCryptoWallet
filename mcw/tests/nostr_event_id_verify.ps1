param(
    [string]$SharedRoot = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',
    [string]$RustBin = '',
    [string]$Python = 'C:\Python314\python.exe'
)
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'This verifier uses the installed Windows native linker; see the handoff for portable commands.' }
$componentRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if (-not $RustBin) { $RustBin = Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin' }
$rustc = Join-Path $RustBin 'rustc.exe'
$rustfmt = Join-Path $RustBin 'rustfmt.exe'
$clippy = Join-Path $RustBin 'clippy-driver.exe'
$rustVersion = (& $rustc --version) -join ''
if ($LASTEXITCODE -or $rustVersion -notmatch '^rustc 1\.99\.0 ') { throw 'Installed Rust 1.99.0 required.' }
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
if (-not $slotHandle) { throw 'Verification deferred: both build slots occupied.' }
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
    $output = Join-Path $componentRoot '.artifacts/nostr-event-id-verification'
    [IO.Directory]::CreateDirectory($output) | Out-Null
    $source = Join-Path $componentRoot 'mcw/src/nostr_event_id.rs'
    $conformance = Join-Path $PSScriptRoot 'nostr_event_id_conformance.rs'
    $oracle = Join-Path $PSScriptRoot 'nostr_event_id_oracle.rs'
    & $rustfmt --edition 2024 --config skip_children=true $source $conformance $oracle
    if ($LASTEXITCODE) { throw 'Rust formatting failed.' }
    & $rustfmt --edition 2024 --config skip_children=true --check $source $conformance $oracle
    if ($LASTEXITCODE) { throw 'Rust formatting check failed.' }
    $harness = Join-Path $output 'lib.rs'
    [IO.File]::WriteAllText($harness, @'
#[path="../../mcw/src/bitcoin_encoding.rs"] pub mod bitcoin_encoding;
#[path="../../mcw/src/json.rs"] pub mod json;
#[path="../../mcw/src/nostr_event_id.rs"] pub mod nostr_event_id;
'@)
    & $rustc --edition=2024 --crate-name nostr_event_id_component --crate-type=lib --emit=metadata -Dwarnings $harness -o (Join-Path $output 'component.rmeta')
    if ($LASTEXITCODE) { throw 'Actual-module warnings check failed.' }
    & $clippy --edition=2024 --crate-name nostr_event_id_component --crate-type=lib --emit=metadata -Dwarnings -Dclippy::all $harness -o (Join-Path $output 'clippy.rmeta')
    if ($LASTEXITCODE) { throw 'Actual-module Clippy check failed.' }
    foreach ($profile in @('debug', 'optimized')) {
        $binary = Join-Path $output "$profile.exe"
        $profileArgs = @('-Ccodegen-units=1')
        if ($profile -eq 'optimized') { $profileArgs += '-Copt-level=2' }
        & $rustc --edition=2024 --crate-name nostr_event_id_conformance --test -Dwarnings @profileArgs $conformance -o $binary
        if ($LASTEXITCODE) { throw "$profile compilation failed." }
        & $binary --test-threads=1 2>&1 | Tee-Object -FilePath (Join-Path $output "$profile-results.txt")
        if ($LASTEXITCODE) { throw "$profile tests failed." }
    }
    $oracleBinary = Join-Path $output 'oracle.exe'
    & $rustc --edition=2024 --crate-name nostr_event_id_oracle -Dwarnings -Ccodegen-units=1 $oracle -o $oracleBinary
    if ($LASTEXITCODE) { throw 'Differential harness compilation failed.' }
    & $Python -X utf8 (Join-Path $PSScriptRoot 'nostr_event_id_reference.py') --binary $oracleBinary --output $output 2>&1 | Tee-Object -FilePath (Join-Path $output 'oracle-results.txt')
    if ($LASTEXITCODE) { throw 'Independent oracle failed.' }
    $hashes = [ordered]@{}
    foreach ($file in @('mcw/src/nostr_event_id.rs','mcw/src/json.rs','mcw/src/json/compat.rs','mcw/src/bitcoin_encoding.rs','mcw/tests/nostr_event_id_conformance.rs','mcw/tests/nostr_event_id_oracle.rs','mcw/tests/nostr_event_id_reference.py','mcw/tests/nostr_event_id_verify.ps1')) {
        $hashes[$file] = (Get-FileHash -LiteralPath (Join-Path $componentRoot $file) -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    [ordered]@{ scope='actual bounded canonical event-ID Rust leaf; synthetic inputs'; status='passed'; rust=$rustVersion; native_target='x86_64-pc-windows-msvc'; format='passed'; warnings='denied; passed'; clippy='all denied; passed'; profiles=@('debug','optimized'); oracle='passed; oracle.json'; source_sha256=$hashes; external_crates=0; remaining='actual retained caller/host incorporation verification is separate'; completed_utc=[DateTime]::UtcNow.ToString('O') } |
        ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $output 'verification.json') -Encoding utf8
    Write-Output ('Evidence: ' + (Join-Path $output 'verification.json'))
} finally {
    $env:CARGO_BUILD_JOBS = $previousJobs
    $slotHandle.Dispose()
}
