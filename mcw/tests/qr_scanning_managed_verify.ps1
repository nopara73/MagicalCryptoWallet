param([string]$SharedRoot='C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',[switch]$CheckPatchesOnly)
$ErrorActionPreference='Stop'
$repoRoot=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$evidence=Join-Path $repoRoot '.artifacts/qr-scanning/managed-harness'
New-Item -ItemType Directory -Force -Path $evidence | Out-Null
$sharedSources=Join-Path $repoRoot '.artifacts/qr-scanning/shared-src'
New-Item -ItemType Directory -Force -Path $sharedSources | Out-Null
$sharedRevision=(& git -C $repoRoot rev-parse origin/master).Trim()
if($LASTEXITCODE -ne 0){throw 'Cannot identify the retained shared bridge revision'}
foreach($export in @(
    @('mcw/src/bridge.rs',(Join-Path $sharedSources 'bridge.rs')),
    @('mcw/src/qr.rs',(Join-Path $sharedSources 'qr.rs')),
    @('mcw/src/qr/tables.rs',(Join-Path $sharedSources 'tables.rs')),
    @('mcw/src/platform.rs',(Join-Path $sharedSources 'platform.rs')),
    @('mcw/src/platform/windows.rs',(Join-Path $sharedSources 'windows.rs')),
    @('mcw/src/app.rs',(Join-Path $repoRoot '.artifacts/qr-scanning/host-source/mcw/src/app.rs')),
    @('mcw/src/lib.rs',(Join-Path $repoRoot '.artifacts/qr-scanning/host-source/mcw/src/lib.rs')),
    @('MagicalCryptoWallet.Client/Application/ManagedApplicationHost.cs',(Join-Path $repoRoot '.artifacts/qr-scanning/ManagedApplicationHost.current.cs')),
    @('MagicalCryptoWallet/Mcw/IMcwApplicationServices.cs',(Join-Path $repoRoot '.artifacts/qr-scanning/IMcwApplicationServices.current.cs'))
)){
    $source=& git -C $repoRoot show ($sharedRevision+':'+$export[0])
    if($LASTEXITCODE -ne 0){throw ('Cannot export the actual shared source: '+$export[0])}
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($export[1])) | Out-Null
    [IO.File]::WriteAllText($export[1],[string]::Join("`n",$source)+"`n",[Text.UTF8Encoding]::new($false))
}
$hostPatch=Join-Path $evidence 'host-lf.patch'
[IO.File]::WriteAllText($hostPatch,[IO.File]::ReadAllText((Join-Path $repoRoot 'Contrib/McwMigration/Patches/qr-scanning-host.patch')).Replace("`r`n","`n"),[Text.UTF8Encoding]::new($false))
$callerPatch=Join-Path $evidence 'caller-lf.patch'
[IO.File]::WriteAllText($callerPatch,[IO.File]::ReadAllText((Join-Path $repoRoot 'Contrib/McwMigration/Patches/qr-scanning-caller.patch')).Replace("`r`n","`n"),[Text.UTF8Encoding]::new($false))
if((Get-Content -LiteralPath (Join-Path $repoRoot '.artifacts/qr-scanning/host-source/mcw/src/app.rs') -Raw) -notmatch 'scan_service::wire'){
    & git -C $repoRoot apply --no-index '--directory=.artifacts/qr-scanning/host-source' $hostPatch
    if($LASTEXITCODE -ne 0){throw 'Concrete scanner host patch does not apply to the retained source'}
}
$caller=Join-Path $repoRoot 'MagicalCryptoWallet.Fluent/Models/UI/QrCodeReader.cs'
if((Get-Content -LiteralPath $caller -Raw) -notmatch 'DecodeCapturedImageAsync'){
    $callerCopy=Join-Path $evidence 'caller/MagicalCryptoWallet.Fluent/Models/UI/QrCodeReader.cs'
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($callerCopy)) | Out-Null
    Copy-Item -LiteralPath $caller -Destination $callerCopy
    & git -C $repoRoot apply --no-index '--directory=.artifacts/qr-scanning/managed-harness/caller' $callerPatch
    if($LASTEXITCODE -ne 0){throw 'Concrete scanner caller patch does not apply to the retained source'}
    $caller=$callerCopy
}
if($CheckPatchesOnly){Write-Output 'Both concrete patches apply to the retained shared source/caller without a build.';return}
$rustBin=Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin'
$handle=$null; $oldPath=$env:PATH; $oldLib=$env:LIB
try {
    if((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory -lt 2097152){throw 'Build deferred: less than 2 GiB free memory'}
    foreach($slot in 1,2){try{$handle=[IO.File]::Open((Join-Path $SharedRoot ('.artifacts/mcw-coordination/build-slot-'+$slot+'.lock')),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None);Write-Host ('QR scanner managed/host acquired build-slot-'+$slot+', verifier PID '+$PID);break}catch [IO.IOException]{}}
    if(-not $handle){throw 'Build deferred: both build slots occupied'}
    $linker=Get-ChildItem 'C:\Program Files\Microsoft Visual Studio\*\*\VC\Tools\MSVC\*\bin\Hostx64\x64\link.exe' | Sort-Object FullName -Descending | Select-Object -First 1
    $msvc=[IO.Path]::GetFullPath((Join-Path $linker.Directory.FullName '../../..'))
    $sdk=Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\Lib' -Directory | Sort-Object Name -Descending | Select-Object -First 1
    $env:PATH=$linker.Directory.FullName+';'+$env:PATH
    $env:LIB=(Join-Path $msvc 'lib/onecore/x64')+';'+(Join-Path $sdk.FullName 'ucrt/x64')+';'+(Join-Path $sdk.FullName 'um/x64')
    $rustc=Join-Path $rustBin 'rustc.exe'
    & $rustc '--edition=2024' '-D' 'warnings' '-C' 'codegen-units=1' '-C' 'overflow-checks=yes' '-C' 'target-feature=+crt-static' '-C' 'opt-level=2' (Join-Path $repoRoot 'mcw/tests/qr_scanning_tools/bridge_fixture.rs') '-o' (Join-Path $evidence 'bridge-fixture.exe')
    if($LASTEXITCODE -ne 0){throw 'Actual Rust decoder/Frame fixture failed to compile'}
    $nuget=Join-Path $env:USERPROFILE '.nuget/packages'
    $references=@{
        'SkiaSharp'=(Join-Path $nuget 'skiasharp/3.119.4/lib/net10.0/SkiaSharp.dll');
        'Avalonia.Base'=(Join-Path $nuget 'avalonia/11.3.22/lib/net8.0/Avalonia.Base.dll');
        'QRackers'=(Join-Path $nuget 'qrackers/1.1.0/lib/netstandard2.1/QRackers.dll');
        'System.Reactive'=(Join-Path $nuget 'system.reactive/6.1.0/lib/net6.0/System.Reactive.dll')
    }
    $xml='<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework><OutputType>Exe</OutputType><AssemblyName>magicalcryptowallet</AssemblyName><UseAppHost>true</UseAppHost><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><LangVersion>14</LangVersion><EnableDefaultCompileItems>false</EnableDefaultCompileItems><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup>'
    foreach($name in $references.Keys){if(-not(Test-Path -LiteralPath $references[$name])){throw ('Missing existing test reference: '+$name)};$xml+='<Reference Include="'+$name+'"><HintPath>'+[Security.SecurityElement]::Escape($references[$name])+'</HintPath></Reference>'}
    foreach($path in @('mcw/tests/qr_scanning_managed_fixture.cs','mcw/tests/qr_scanning_tools/termination_type.cs','.artifacts/qr-scanning/ManagedApplicationHost.current.cs','MagicalCryptoWallet/Mcw/Scanning/McwQrDecoder.cs','.artifacts/qr-scanning/IMcwApplicationServices.current.cs')){
        $xml+='<Compile Include="'+[Security.SecurityElement]::Escape((Join-Path $repoRoot $path))+'" />'
    }
    $xml+='<Compile Include="'+[Security.SecurityElement]::Escape($caller)+'" />'
    $xml+='</ItemGroup></Project>'
    $project=Join-Path $evidence 'ScanCaller.csproj';[IO.File]::WriteAllText($project,$xml,[Text.UTF8Encoding]::new($false))
    $config=Join-Path $evidence 'NuGet.Config';[IO.File]::WriteAllText($config,'<configuration><packageSources><clear /></packageSources></configuration>')
    & dotnet restore $project '--configfile' $config '-p:ImportDirectoryBuildProps=false' '-p:ImportDirectoryBuildTargets=false' '-p:ImportDirectoryPackagesProps=false' '--disable-parallel'
    if($LASTEXITCODE -ne 0){throw 'Offline managed fixture restore failed'}
    & dotnet build $project '--no-restore' '-c' 'Release' '-m:1' '--disable-build-servers' '-p:ImportDirectoryBuildProps=false' '-p:ImportDirectoryBuildTargets=false' '-p:ImportDirectoryPackagesProps=false'
    if($LASTEXITCODE -ne 0){throw 'Actual production QrCodeReader/adapter compilation failed'}
    $output=Join-Path $evidence 'bin/Release/net10.0'
    Copy-Item -LiteralPath (Join-Path $nuget 'skiasharp.nativeassets.win32/3.119.4/runtimes/win-x64/native/libSkiaSharp.dll') -Destination $output
    $log=Join-Path $evidence 'caller-result.json'
    $retained=Join-Path $repoRoot 'MagicalCryptoWallet.Tests/UnitTests/QrDecode/QrResources'
    & dotnet (Join-Path $output 'magicalcryptowallet.dll') (Join-Path $evidence 'bridge-fixture.exe') (Join-Path $repoRoot '.artifacts/qr-scanning/content-final/corpus.json') $retained | Tee-Object -FilePath $log
    if($LASTEXITCODE -ne 0){throw 'Actual production capture decode leaf test failed'}
    $hostExe=Join-Path $output 'host-fixture.exe'
    & $rustc '--edition=2024' '-D' 'warnings' '-C' 'codegen-units=1' '-C' 'overflow-checks=yes' '-C' 'target-feature=+crt-static' '-C' 'opt-level=2' (Join-Path $repoRoot 'mcw/tests/qr_scanning_tools/host_fixture.rs') '-o' $hostExe
    if($LASTEXITCODE -ne 0){throw 'Actual patched host fixture failed to compile'}
    $hostResult=Join-Path $evidence 'host-result.json'
    & $hostExe (Join-Path $repoRoot '.artifacts/qr-scanning/content-final/corpus.json') $hostResult $retained
    if($LASTEXITCODE -ne 0){throw 'Actual managed transport / patched host decode test failed'}
    $hashes=@{}
    foreach($path in @('MagicalCryptoWallet/Mcw/Scanning/McwQrDecoder.cs','mcw/tests/qr_scanning_managed_fixture.cs','mcw/tests/qr_scanning_tools/bridge_fixture.rs','mcw/tests/qr_scanning_tools/host_fixture.rs','mcw/tests/qr_scanning_tools/termination_type.cs','mcw/src/scan_service/wire.rs','mcw/src/scan_service/decoder.rs','.artifacts/qr-scanning/shared-src/bridge.rs','.artifacts/qr-scanning/ManagedApplicationHost.current.cs','.artifacts/qr-scanning/host-source/mcw/src/app.rs','Contrib/McwMigration/Patches/qr-scanning-host.patch','Contrib/McwMigration/Patches/qr-scanning-caller.patch')){
        $hashes[$path]=(Get-FileHash -LiteralPath (Join-Path $repoRoot $path) -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    $hashes['patched_production_caller']=(Get-FileHash -LiteralPath $caller -Algorithm SHA256).Hash.ToLowerInvariant()
    Get-ChildItem -LiteralPath (Join-Path $repoRoot 'mcw/src/scan_service') -Filter '*.rs' | ForEach-Object {$hashes[$_.Name]=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}
    @{source_hashes=$hashes;shared_revision=$sharedRevision;result=(Get-Content -LiteralPath $log -Raw | ConvertFrom-Json);host_result=(Get-Content -LiteralPath $hostResult -Raw | ConvertFrom-Json);fixture_binary_sha256=(Get-FileHash -LiteralPath (Join-Path $evidence 'bridge-fixture.exe') -Algorithm SHA256).Hash.ToLowerInvariant();host_fixture_sha256=(Get-FileHash -LiteralPath $hostExe -Algorithm SHA256).Hash.ToLowerInvariant();reference_role='Dev-only actual Frame/decoder fixture and patched host; retained Skia image boundary; no shipping package/camera claim'} |
        ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $evidence 'evidence.json')
} finally {$env:PATH=$oldPath;$env:LIB=$oldLib;if($handle){$handle.Dispose()}}
