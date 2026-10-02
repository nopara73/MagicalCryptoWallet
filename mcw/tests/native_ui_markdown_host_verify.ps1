param([string]$SharedRoot='C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',[switch]$ProposedRegistration,[string]$OutputDirectory)
$ErrorActionPreference='Stop'
$componentRoot=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$output=if($OutputDirectory){[IO.Path]::GetFullPath($OutputDirectory)}else{Join-Path $componentRoot ('.artifacts/native-ui-markdown-verification/host-runs/'+[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ'))}
[IO.Directory]::CreateDirectory($output)|Out-Null
if($ProposedRegistration){
    $hostSource=Join-Path $output 'host-source'
    & python (Join-Path $PSScriptRoot 'native_ui_markdown_host_prepare.py') --output $hostSource
    if($LASTEXITCODE){throw 'Proposed host snapshot failed'}
    $manifest=Join-Path $output 'host-source/Cargo.toml'
}else{
    $manifest=Join-Path $componentRoot 'mcw/Cargo.toml'
    if(-not (Select-String -LiteralPath (Join-Path $componentRoot 'mcw/src/lib.rs') -SimpleMatch 'pub mod markdown;')){throw 'Production Markdown module is not registered'}
}
if(([double](Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory/1MB)-lt 2){throw 'Verification deferred: less than 2 GiB free memory'}
$slotHandle=$null
foreach($slot in 1..2){try{$slotHandle=[IO.File]::Open((Join-Path $SharedRoot ".artifacts/mcw-coordination/build-slot-$slot.lock"),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None);break}catch [IO.IOException]{}}
if(-not $slotHandle){throw 'Verification deferred: both build slots occupied'}
Write-Output "Build slot $slot; verifier PID $PID; bounded real Markdown host proof"
try{
    $rustBin=Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin'
    $env:PATH=$rustBin+';'+$env:PATH
    $env:CARGO_BUILD_JOBS='1'
    $env:CARGO_TARGET_DIR=Join-Path $output 'host-build'
    if(-not $env:VCToolsInstallDir){
        $vswhere=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
        $vsRoot=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        & (Join-Path $vsRoot 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation *> $null
    }
    $probeProject=Join-Path $componentRoot 'Contrib/McwMigration/NativeUiBridgeProbe/NativeUiBridgeProbe.csproj'
    $callerProject=Join-Path $componentRoot 'MagicalCryptoWallet.Fluent/MagicalCryptoWallet.Fluent.csproj'
    & dotnet restore $probeProject --disable-parallel --locked-mode -p:Configuration=Release -m:1
    if($LASTEXITCODE){throw 'Managed host probe restore failed'}
    & dotnet restore $callerProject --disable-parallel --locked-mode -p:Configuration=Release -m:1
    if($LASTEXITCODE){throw 'Caller restore failed'}
    $bindingArguments=@()
    if($ProposedRegistration){$bindingArguments=@('--host-source',$hostSource)}
    & python (Join-Path $PSScriptRoot 'native_ui_markdown_bindings.py') @bindingArguments --write (Join-Path $output 'compiler-inputs-before.json')
    if($LASTEXITCODE){throw 'Actual host compiler input binding failed'}
    & (Join-Path $rustBin 'cargo.exe') build --offline --locked --manifest-path $manifest
    if($LASTEXITCODE){throw 'Real Rust host build failed'}
    $hostBin=Join-Path $output 'host-bin'
    & dotnet build $probeProject -c Release --no-restore -m:1 -o $hostBin
    if($LASTEXITCODE){throw 'Shipping managed host/typed adapter probe build failed'}
    Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'debug/mcw.exe') -Destination (Join-Path $hostBin 'mcw.exe')
    # Rename only the test apphost. Its embedded DLL remains uniquely named,
    # avoiding both NuGet project-name and CLR assembly-name collisions.
    Copy-Item -LiteralPath (Join-Path $hostBin 'NativeUiBridgeProbe.exe') -Destination (Join-Path $hostBin 'magicalcryptowallet.exe')
    $report=Join-Path $output 'host-report.json'
    $inputs=Join-Path $componentRoot '.artifacts/native-ui-markdown-inputs'
    # GUI-subsystem executables require explicit waiting even in a shell probe.
    $quotedReport='"'+$report+'"';$quotedInputs='"'+$inputs+'"'
    $process=Start-Process -FilePath (Join-Path $hostBin 'mcw.exe') -ArgumentList @('gui',$quotedReport,$quotedInputs) -WindowStyle Hidden -RedirectStandardError (Join-Path $output 'host-stderr.txt') -PassThru
    try{
        if(-not $process.WaitForExit(60000)){throw 'Synthetic Markdown host proof timed out; process remains preserved'}
        if($process.ExitCode -ne 0){throw ('Synthetic Markdown host proof failed: '+$process.ExitCode)}
    }finally{$process.Dispose()}
    $result=Get-Content -LiteralPath $report -Raw|ConvertFrom-Json
    if(-not $result.realHost -or $result.documents -ne 13 -or $result.malformedRejected -ne 4 -or $result.adversarialLimits -ne 2 -or -not $result.canceled -or $result.concurrent -ne 12 -or $result.nativeInProgressCancellationVerified){throw 'Runtime report incomplete'}
    Get-Content -LiteralPath $report
    & python (Join-Path $PSScriptRoot 'native_ui_markdown_legacy_compare.py') --output $output --host
    if($LASTEXITCODE){throw 'Actual host corpus comparison failed'}
    & dotnet build $callerProject -c Release --no-restore -m:1
    if($LASTEXITCODE){throw 'Retained release-highlights production caller build failed'}
    & python (Join-Path $PSScriptRoot 'native_ui_markdown_bindings.py') @bindingArguments --check (Join-Path $output 'compiler-inputs-before.json') --report (Join-Path $output 'compiler-inputs-after.json')
    if($LASTEXITCODE){throw 'Actual host compiler inputs changed during verification'}
    Get-FileHash -LiteralPath (Join-Path $hostBin 'mcw.exe'),(Join-Path $hostBin 'NativeUiBridgeProbe.dll') -Algorithm SHA256|Select-Object Path,Hash|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $output 'built-binary-hashes.json') -Encoding utf8
    [ordered]@{proposed_registration=[bool]$ProposedRegistration;production_registered=(-not [bool]$ProposedRegistration);native_release_audited=$false;native_in_progress_cancellation_verified=$false;actual_managed_host_used=$true;compiler_inputs_before='compiler-inputs-before.json';compiler_inputs_after='compiler-inputs-after.json';completed_utc=[DateTime]::UtcNow.ToString('O')}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $output 'host-verification.json') -Encoding utf8
    Write-Output "Candidate real-host evidence: $output"
}finally{$slotHandle.Dispose()}
