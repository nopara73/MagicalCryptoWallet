param([string]$Snapshot,
    [string]$SharedRoot='C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',
    [string]$Python='C:\Python314\python.exe')
$ErrorActionPreference='Stop'
if(-not $Snapshot){throw 'Prepare a fresh actual-source snapshot before acquiring a build slot'}
$Snapshot=[IO.Path]::GetFullPath($Snapshot)
$repo=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if(-not $Snapshot.StartsWith((Join-Path $repo '.artifacts/'),[StringComparison]::OrdinalIgnoreCase)){throw 'Snapshot outside this task artifacts'}
$handle=$null
$variables=@('PATH','LIB','INCLUDE','CARGO','CARGO_HOME','RUSTUP_HOME','CARGO_TARGET_DIR','CARGO_BUILD_JOBS','RUSTFLAGS','RUSTC_BOOTSTRAP')
$previous=@{}
foreach($variable in $variables){$previous[$variable]=[Environment]::GetEnvironmentVariable($variable,'Process')}
try{
    if((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory -lt 2097152){throw 'Build deferred: RAM below 2 GiB'}
    foreach($slot in 1,2){
        try{$handle=[IO.File]::Open((Join-Path $SharedRoot ('.artifacts/mcw-coordination/build-slot-'+$slot+'.lock')),
            [IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None);break}
        catch [IO.IOException] { }
    }
    if(-not $handle){throw 'Build deferred: slots occupied'}
    Write-Output ('BUILD SLOT '+$slot+'; verifier PID '+$PID+'; existing bounded content actual-host verification')
    if(-not $env:VCToolsInstallDir){
        $vswhere=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
        $vsRoot=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if(-not $vsRoot){throw 'Existing Visual Studio C++ build tools required'}
        & (Join-Path $vsRoot 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
    }
    $toolchain=Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin'
    $env:PATH=$toolchain+';'+$env:PATH
    $env:CARGO=Join-Path $toolchain 'cargo.exe'
    $env:CARGO_HOME=Join-Path $SharedRoot '.artifacts/mcw-tools/cargo'
    $env:RUSTUP_HOME=Join-Path $SharedRoot '.artifacts/mcw-tools/rustup'
    $env:CARGO_TARGET_DIR=Join-Path $Snapshot '.artifacts/mcw-build'
    $env:CARGO_BUILD_JOBS='1'
    $env:RUSTFLAGS=$null
    $env:RUSTC_BOOTSTRAP=$null
    $manifest=Join-Path $Snapshot 'mcw/Cargo.toml'
    # Lint the shipping library/executable only. Published standalone component
    # harnesses are independently compiled with warnings denied by their owners.
    & $env:CARGO clippy --manifest-path $manifest --lib --bin mcw --locked --offline -- -D warnings
    if($LASTEXITCODE){throw 'Actual shipping content registration Clippy failed'}
    $managedProject=Join-Path $Snapshot 'ContentActualHost/ContentActualHost.csproj'
    & dotnet build $managedProject --nologo --verbosity minimal --configuration Release -m:1 -p:UseSharedCompilation=false
    if($LASTEXITCODE){throw 'Actual Core/caller/managed host compilation failed'}
    $output=Join-Path $Snapshot 'ContentActualHost/bin/Release/net10.0'
    # The test child has a unique assembly name to preserve the actual Core DLL.
    # Its apphost keeps that DLL name when copied to the native host's GUI alias.
    Copy-Item -LiteralPath (Join-Path $output 'McwContentActualHostFixture.exe') -Destination (Join-Path $output 'magicalcryptowallet.exe')
    & $Python (Join-Path $Snapshot 'Contrib/Mcw/build.py') --rid win-x64 --copy-to $output
    if($LASTEXITCODE){throw 'Actual native OS-runtime host build/audit failed'}
    & $Python (Join-Path $repo 'mcw/tests/compression_content_host_run.py') --snapshot $Snapshot
    if($LASTEXITCODE){throw 'Actual host retained caller verification failed'}
}finally{
    if($handle){$handle.Dispose();Write-Output ('BUILD SLOT '+$slot+' released; verifier PID '+$PID)}
    foreach($variable in $variables){[Environment]::SetEnvironmentVariable($variable,$previous[$variable],'Process')}
}
