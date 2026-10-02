param(
    [string]$SharedRoot='C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',
    [string]$RustBin=''
)
$ErrorActionPreference='Stop'
$taskScope=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if (-not $RustBin) { $RustBin=Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin' }
$taskCompiler=Join-Path $RustBin 'rustc.exe'
$taskVersion=(& $taskCompiler --version) -join ''
if ($LASTEXITCODE -or $taskVersion -notmatch '^rustc 1\.99\.0 ') { throw 'Rust 1.99.0 required.' }
$taskFreeGiB=[double](Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory/1MB
if ($taskFreeGiB -lt 2) { throw 'Deferred: fewer than 2 GiB free.' }
$taskBuildLock=$null
foreach ($taskSlot in 1..2) {
    try { $taskBuildLock=[IO.File]::Open((Join-Path $SharedRoot ".artifacts/mcw-coordination/build-slot-$taskSlot.lock"),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None); break } catch [IO.IOException] { }
}
if (-not $taskBuildLock) { throw 'Deferred: both build slots occupied.' }
try {
    if (-not $env:VCToolsInstallDir) {
        $taskVswhere=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
        $taskVsRoot=& $taskVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if (-not $taskVsRoot) { throw 'Installed native linker unavailable.' }
        & (Join-Path $taskVsRoot 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
    }
    $taskOutput=Join-Path $taskScope '.artifacts/privacy-control-verification'
    [IO.Directory]::CreateDirectory($taskOutput) | Out-Null
    $taskModule=Join-Path $taskScope 'mcw/src/privacy_service/control_codec/mod.rs'
    $taskService=Join-Path $taskScope 'mcw/src/privacy_service/control_codec/service.rs'
    $taskStream=Join-Path $taskScope 'mcw/src/privacy_service/control_codec/stream.rs'
    $taskTest=Join-Path $PSScriptRoot 'privacy_control.rs'
    & (Join-Path $RustBin 'rustfmt.exe') --edition 2024 --check $taskModule $taskService $taskStream $taskTest
    if ($LASTEXITCODE) { throw 'Formatting failed.' }
    & (Join-Path $RustBin 'clippy-driver.exe') --edition=2024 --crate-type=lib --emit=metadata -Dwarnings -Dclippy::all $taskModule -o (Join-Path $taskOutput 'control-clippy.rmeta')
    if ($LASTEXITCODE) { throw 'Codec Clippy failed.' }
    $taskExecutable=Join-Path $taskOutput 'privacy_control.exe'
    & $taskCompiler --edition=2024 --test -Dwarnings $taskTest -o $taskExecutable
    if ($LASTEXITCODE) { throw 'Codec test compilation failed.' }
    & $taskExecutable --test-threads=1 2>&1 | Tee-Object -FilePath (Join-Path $taskOutput 'rust-tests.txt')
    if ($LASTEXITCODE) { throw 'Codec conformance failed.' }
    [ordered]@{
        scope='bounded Tor control reply/CRLF codec'
        rust=$taskVersion
        target='x86_64-pc-windows-msvc'
        tests=12
        oracle_replies=24
        dotnet_status_oracle_cases=8000
        verified_utc=[DateTime]::UtcNow.ToString('O')
        shipping_import_audit=$false
        production_master_cutover=$false
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskOutput 'codec-verification.json') -Encoding utf8
    Write-Output "Codec evidence saved: $taskOutput"
} finally { $taskBuildLock.Dispose() }
