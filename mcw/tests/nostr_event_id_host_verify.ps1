param(
    [string]$SharedRoot = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',
    [string]$Root = '',
    [string]$RustBin = '',
    [string]$Python = 'C:\Python314\python.exe'
)
$ErrorActionPreference = 'Stop'
if (-not $Root) { $Root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..')) }
if (-not $RustBin) { $RustBin = Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin' }
if (-not (Test-Path -LiteralPath (Join-Path $Root 'mcw/Cargo.toml'))) { throw 'Published application host required.' }
if (Test-Path -LiteralPath (Join-Path $Root 'MagicalCryptoWallet/Mcw/Nostr/McwNostrUpdateClient.cs')) { throw 'Do not compile the preserved broad Nostr draft.' }
$freeGiB = [double](Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB
if ($freeGiB -lt 2) { throw 'Verification deferred: fewer than 2 GiB free.' }
$slotHandle = $null
foreach ($slot in 1..2) {
    try {
        $slotPath = Join-Path $SharedRoot ".artifacts/mcw-coordination/build-slot-$slot.lock"
        $slotHandle = [IO.File]::Open($slotPath,[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
        break
    } catch [IO.IOException] { }
}
if (-not $slotHandle) { throw 'Verification deferred: both build slots occupied.' }
$previousPath = $env:PATH
$previousJobs = $env:CARGO_BUILD_JOBS
$previousRustc = $env:RUSTC
$previousRuntime = $env:MCW_WINDOWS_RUNTIME
try {
    Write-Output ('Build slot ' + $slot + '; free memory GiB ' + $freeGiB.ToString('F2'))
    $env:PATH = $RustBin + ';' + $env:PATH
    $env:RUSTC = Join-Path $RustBin 'rustc.exe'
    $env:CARGO_BUILD_JOBS = '1'
    $env:MCW_WINDOWS_RUNTIME = '0'
    if (-not $env:VCToolsInstallDir) {
        $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
        $vsRoot = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if (-not $vsRoot) { throw 'Installed native linker unavailable.' }
        & (Join-Path $vsRoot 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
    }
    $output = Join-Path $Root '.artifacts/nostr-event-id-host-verification'
    [IO.Directory]::CreateDirectory($output) | Out-Null
    & (Join-Path $RustBin 'cargo.exe') build --manifest-path (Join-Path $Root 'mcw/Cargo.toml') --offline --locked --bin mcw --target-dir (Join-Path $output 'native-build') -j 1 2>&1 | Tee-Object -FilePath (Join-Path $output 'rust-build.txt')
    if ($LASTEXITCODE) { throw 'Actual mcw debug build failed.' }
    $project = Join-Path $Root 'mcw/tests/nostr_host/NostrHost.csproj'
    & dotnet restore $project --locked-mode --disable-parallel -p:BuildInParallel=false 2>&1 | Tee-Object -FilePath (Join-Path $output 'managed-restore.txt')
    if ($LASTEXITCODE) { throw 'Retained project locked restore failed.' }
    & dotnet build $project --no-restore -c Release -m:1 -nr:false -p:BuildInParallel=false -p:UseSharedCompilation=false 2>&1 | Tee-Object -FilePath (Join-Path $output 'managed-build.txt')
    if ($LASTEXITCODE) { throw 'Actual core/caller/host-adapter test build failed.' }
    $childDirectory = Join-Path $Root 'mcw/tests/nostr_host/bin/Release/net10.0'
    $native = Join-Path $childDirectory 'mcw.exe'
    [IO.File]::WriteAllBytes($native,[IO.File]::ReadAllBytes((Join-Path $output 'native-build/debug/mcw.exe')))
    $recordPath = Join-Path $output 'host-results.json'
    & $Python -X utf8 (Join-Path $Root 'mcw/tests/nostr_host/run.py') --host $native --evidence $recordPath --log (Join-Path $output 'host-run.txt')
    if ($LASTEXITCODE) { throw 'Real-host synthetic caller tests failed.' }
    if (-not (Test-Path -LiteralPath $recordPath)) { throw 'Host caller evidence missing.' }
    $record = Get-Content -LiteralPath $recordPath -Raw | ConvertFrom-Json
    if ($record.status -ne 'passed' -or -not $record.actual_native_host -or -not $record.actual_retained_update_caller) { throw 'Real caller tests did not pass.' }
    $hashes = [ordered]@{}
    foreach ($file in @('mcw/src/nostr_event_id.rs','mcw/src/json.rs','mcw/src/bitcoin_encoding.rs','mcw/src/app.rs','mcw/src/lib.rs','MagicalCryptoWallet/Mcw/Nostr/McwNostrEventId.cs','MagicalCryptoWallet/WebClients/MagicalCryptoWalletNostrClient.cs','MagicalCryptoWallet/Mcw/IMcwApplicationServices.cs','MagicalCryptoWallet.Client/Application/ManagedApplicationHost.cs','mcw/tests/nostr_host/Program.cs','mcw/tests/nostr_host/NostrHost.csproj','mcw/tests/nostr_host/run.py','mcw/tests/nostr_event_id_host_verify.ps1')) {
        $hashes[$file] = (Get-FileHash -LiteralPath (Join-Path $Root $file) -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    [ordered]@{status='passed';scope='actual mcw + retained managed Nostr update caller, synthetic local process only';source_root=$Root;native_target='x86_64-pc-windows-msvc';native_profile='debug; component integration only';managed_profile='Release net10.0; warnings as errors';source_sha256=$hashes;host_binary_sha256=(Get-FileHash -LiteralPath $native -Algorithm SHA256).Hash.ToLowerInvariant();host_tests=$record;additional_shipping_executables=0;not_claimed='NNostr package removal, full Nostr/transport migration, native release or five-target acceptance';completed_utc=[DateTime]::UtcNow.ToString('O')} |
        ConvertTo-Json -Depth 7 | Set-Content -LiteralPath (Join-Path $output 'verification.json') -Encoding utf8
    Write-Output ('Evidence: ' + (Join-Path $output 'verification.json'))
} finally {
    $env:PATH = $previousPath
    $env:CARGO_BUILD_JOBS = $previousJobs
    $env:RUSTC = $previousRustc
    $env:MCW_WINDOWS_RUNTIME = $previousRuntime
    $slotHandle.Dispose()
}
