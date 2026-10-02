param([string]$SharedRoot='C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',
    [string]$Python='C:\Python314\python.exe', [int]$WaitForSlotSeconds=0)
$ErrorActionPreference='Stop'
$repo=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$root=Join-Path $repo '.artifacts/compression-content'
New-Item -ItemType Directory -Path $root -Force | Out-Null
$rustc=Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin/rustc.exe'
$handle=$null
$oldPath=$env:PATH
$oldLib=$env:LIB
try {
    if((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory -lt 2097152){throw 'Build deferred: RAM below 2 GiB'}
    $waiting=[Diagnostics.Stopwatch]::StartNew()
    do{
        foreach($slot in 1,2){
            try {$handle=[IO.File]::Open((Join-Path $SharedRoot ('.artifacts/mcw-coordination/build-slot-'+$slot+'.lock')),
                [IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None); break}
            catch [IO.IOException] { }
        }
        if($handle -or $waiting.Elapsed.TotalSeconds -ge $WaitForSlotSeconds){break}
        Write-Output ('Existing content verification waiting; no build slot held; verifier PID '+$PID)
        Start-Sleep -Seconds 30
        if((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory -lt 2097152){throw 'Build deferred: RAM below 2 GiB'}
    }while($true)
    if(-not $handle){throw 'Build deferred: slots occupied'}
    Write-Output ('BUILD SLOT '+$slot+'; verifier PID '+$PID+'; existing bounded content codec/component verification')
    $version=& $rustc --version
    if($LASTEXITCODE -ne 0 -or $version -notlike 'rustc 1.99.0 *'){throw 'Rust 1.99.0 required'}
    $linker=Get-ChildItem -Path 'C:\Program Files\Microsoft Visual Studio\*\*\VC\Tools\MSVC\*\bin\Hostx64\x64\link.exe' | Sort-Object FullName -Descending | Select-Object -First 1
    $msvc=[IO.Path]::GetFullPath((Join-Path $linker.Directory.FullName '../../..'))
    $sdk=Get-ChildItem -LiteralPath 'C:\Program Files (x86)\Windows Kits\10\Lib' -Directory | Sort-Object Name -Descending | Select-Object -First 1
    $env:PATH=$linker.Directory.FullName+';'+$env:PATH
    $env:LIB=(Join-Path $msvc 'lib/onecore/x64')+';'+(Join-Path $sdk.FullName 'ucrt/x64')+';'+(Join-Path $sdk.FullName 'um/x64')
    $sourceFiles=@('mcw/src/compression.rs','mcw/src/http1.rs','mcw/src/content_service/mod.rs','mcw/src/content_service/brotli.rs','mcw/src/content_service/adapter.rs',
        'mcw/src/content_service/data/.gitattributes','mcw/src/content_service/data/dictionary.bin','mcw/src/content_service/data/tables.rs','mcw/src/content_service/data/manifest.json',
        'mcw/src/content_service/data/PROVENANCE.md','mcw/src/content_service/data/RFC-DATA-LICENSE',
        'mcw/tests/compression_content_conformance.rs','mcw/tests/compression_content_reference.py',
        'mcw/tests/compression_content_verify.ps1','mcw/tests/compression_brotli_tables.py','mcw/tests/compression_content_adapter_tests.cs',
        'MagicalCryptoWallet/Mcw/IMcwApplicationServices.cs','MagicalCryptoWallet/Mcw/Content/McwContentDecoder.cs','MagicalCryptoWallet/Mcw/Content/McwContentDecodingHandler.cs')
    $sourceHashes=[ordered]@{}
    foreach($source in $sourceFiles){$sourceHashes[$source]=(Get-FileHash -LiteralPath (Join-Path $repo $source) -Algorithm SHA256).Hash.ToLowerInvariant()}
    $tests=@{}
    foreach($profile in @(@('debug','0'),@('optimized','2'))){
        $testExe=Join-Path $root ($profile[0]+'-conformance.exe')
        & $rustc --edition=2024 --test -D warnings -C codegen-units=1 -C overflow-checks=yes -C target-feature=+crt-static -C ('opt-level='+$profile[1]) (Join-Path $repo 'mcw/tests/compression_content_conformance.rs') -o $testExe
        if($LASTEXITCODE -ne 0){throw ($profile[0]+' conformance compilation failed')}
        $testOutput=& $testExe --test-threads=1 2>&1
        $testExit=$LASTEXITCODE
        $testOutput | Set-Content -LiteralPath (Join-Path $root ($profile[0]+'.log')) -Encoding utf8
        $testOutput | Write-Output
        if($testExit -ne 0){throw ($profile[0]+' content/HTTP conformance failed')}
        $tests[$profile[0]]=@{passed=$true;result=(@($testOutput | Select-String -Pattern '^test result:') -join ' ')}
    }
    $driver=@'
#![forbid(unsafe_code)]
#[path="COMPRESSION"] pub mod compression;
#[path="CONTENT"] pub mod content_service;
use std::io::{self,BufRead};
fn unhex(v:&str)->Vec<u8>{v.as_bytes().chunks_exact(2).map(|b|u8::from_str_radix(std::str::from_utf8(b).unwrap(),16).unwrap()).collect()}
fn hex(v:&[u8])->String{v.iter().map(|b|format!("{b:02x}")).collect()}
fn main(){
    for line in io::stdin().lock().lines(){
        let line=line.unwrap();let fields:Vec<_>=line.split('\t').collect();let input=unhex(fields[2]);
        let mut calls=0;let abort=fields.get(3).and_then(|v|v.parse::<usize>().ok()).unwrap_or(usize::MAX);
        let mut check=||{calls+=1;if calls>abort{Err(content_service::Abort::Cancelled)}else{Ok(())}};
        if fields[0]=="P"{
            let p=content_service::adapter::execute(&input,&mut check).unwrap();println!("PACKET\t{}",hex(&p));
        }else if fields[0]=="B"{
            match content_service::brotli::decode(&input,compression::Limits::default(),4096,compression::TrailingData::Reject,&mut check){
                Ok(d)=>println!("OK\t{}\t{}\t{}\t{}",d.consumed,d.work,d.meta_blocks,hex(&d.bytes)),
                Err(e)=>println!("ERR\t{:?}\t{}\t{}",e.kind,e.input_consumed,e.output_produced),
            }
        }else{
            let encodings:Vec<&[u8]>=if fields[1]=="-"{vec![]}else{vec![fields[1].as_bytes()]};
            match content_service::decode(&input,&encodings,content_service::Limits::default(),&mut check){
                Ok(d)=>println!("OK\t{}\t{}\t{}\t{}",d.encoded_len,d.layers.iter().map(|l|l.work).sum::<u64>(),d.layers.len(),hex(&d.bytes)),
                Err(e)=>println!("ERR\t{:?}\t{}\t{}",e.kind,e.input_consumed,e.output_produced),
            }
        }
    }
}
'@
    $driver=$driver.Replace('COMPRESSION',(Join-Path $repo 'mcw/src/compression.rs').Replace('\','/')).Replace('CONTENT',(Join-Path $repo 'mcw/src/content_service/mod.rs').Replace('\','/'))
    $driverSource=Join-Path $root 'actual-source-driver.rs'
    $driverExe=Join-Path $root 'actual-source-driver.exe'
    [IO.File]::WriteAllText($driverSource,$driver)
    & $rustc --edition=2024 -D warnings -C codegen-units=1 -C overflow-checks=yes -C opt-level=2 -C target-feature=+crt-static $driverSource -o $driverExe
    if($LASTEXITCODE -ne 0){throw 'Actual-source content driver compilation failed'}
    $oracle=Join-Path $root 'oracle'
    New-Item -ItemType Directory -Path $oracle -Force | Out-Null
    $project=Join-Path $oracle 'compression-oracle.csproj'
    [IO.File]::WriteAllText((Join-Path $oracle 'Directory.Build.props'),'<Project />')
    [IO.File]::WriteAllText((Join-Path $oracle 'Directory.Build.targets'),'<Project />')
    [IO.File]::WriteAllText($project,'<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><EnableDefaultCompileItems>false</EnableDefaultCompileItems><RestoreSources></RestoreSources></PropertyGroup><ItemGroup><Compile Include="Program.cs" /></ItemGroup></Project>')
    $managed=@'
using System.Buffers;
using System.IO.Compression;
using System.Globalization;
while(Console.ReadLine() is { } line){
    var f=line.Split('\t');
    try{
        if(f[0]=="C"){
            var data=Convert.FromHexString(f[3]);var output=new byte[BrotliEncoder.GetMaxCompressedLength(data.Length)];
            if(!BrotliEncoder.TryCompress(data,output,out var count,int.Parse(f[1],CultureInfo.InvariantCulture),int.Parse(f[2],CultureInfo.InvariantCulture)))throw new InvalidDataException("encode failed");
            Console.WriteLine("OK\t"+Convert.ToHexString(output.AsSpan(0,count)).ToLowerInvariant());
        }else{
            var data=Convert.FromHexString(f[1]);var output=new byte[2*1024*1024];using var decoder=new BrotliDecoder();
            var status=decoder.Decompress(data,output,out var consumed,out var written);
            if(status!=OperationStatus.Done){Console.WriteLine("ERR\t"+status);continue;}
            Console.WriteLine("OK\t"+consumed+"\t"+Convert.ToHexString(output.AsSpan(0,written)).ToLowerInvariant());
        }
    }catch(Exception e){Console.WriteLine("ERR\t"+e.GetType().Name);}
}
'@
    [IO.File]::WriteAllText((Join-Path $oracle 'Program.cs'),$managed)
    $empty=Join-Path $root 'empty-nuget'
    New-Item -ItemType Directory -Path $empty -Force | Out-Null
    & dotnet build $project --nologo --verbosity quiet --configuration Release -m:1 -p:RestoreSources=$empty -p:UseSharedCompilation=false
    if($LASTEXITCODE -ne 0){throw 'Independent .NET Brotli oracle build failed'}
    $oracleDll=Join-Path $oracle 'bin/Release/net10.0/compression-oracle.dll'
    & $Python (Join-Path $repo 'mcw/tests/compression_content_reference.py') --driver $driverExe --oracle $oracleDll --evidence $root
    if($LASTEXITCODE -ne 0){throw 'Content differential verification failed'}
    $managedRoot=Join-Path $root 'managed-adapter'
    New-Item -ItemType Directory -Path $managedRoot -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $managedRoot 'Directory.Build.props'),'<Project />')
    [IO.File]::WriteAllText((Join-Path $managedRoot 'Directory.Build.targets'),'<Project />')
    $links=@('MagicalCryptoWallet/Mcw/IMcwApplicationServices.cs','MagicalCryptoWallet/Mcw/Content/McwContentDecoder.cs',
        'MagicalCryptoWallet/Mcw/Content/McwContentDecodingHandler.cs','mcw/tests/compression_content_adapter_tests.cs')
    $xml='<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><LangVersion>14</LangVersion><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><EnableDefaultCompileItems>false</EnableDefaultCompileItems><RestoreSources></RestoreSources></PropertyGroup><ItemGroup>'
    foreach($link in $links){$xml+='<Compile Include="'+[Security.SecurityElement]::Escape((Join-Path $repo $link))+'" />'}
    $xml+='</ItemGroup></Project>'
    $managedProject=Join-Path $managedRoot 'managed-adapter.csproj'
    [IO.File]::WriteAllText($managedProject,$xml)
    & dotnet build $managedProject --nologo --verbosity quiet --configuration Release -m:1 -p:RestoreSources=$empty -p:UseSharedCompilation=false
    if($LASTEXITCODE -ne 0){throw 'Actual managed content leaf compilation failed'}
    $managedLog=& dotnet (Join-Path $managedRoot 'bin/Release/net10.0/managed-adapter.dll') $driverExe 2>&1
    $managedExit=$LASTEXITCODE
    $managedLog | Set-Content -LiteralPath (Join-Path $root 'managed-adapter.log') -Encoding utf8
    $managedLog | Write-Output
    if($managedExit -ne 0 -or (@($managedLog | Select-String -Pattern '^COMPONENT TEST RESULT: 14 passed; actual_application_host=false').Count -ne 1)){throw 'Managed content component verification failed'}
    foreach($source in $sourceFiles){
        if((Get-FileHash -LiteralPath (Join-Path $repo $source) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $sourceHashes[$source]){throw ('Source changed during verification: '+$source)}
    }
    $record=@{compiler=$version;platform='x86_64-pc-windows-msvc';edition=2024;source_hashes=$sourceHashes;tests=$tests;managed_component_passed=14;actual_application_host=$false;differential=(Get-Content -LiteralPath (Join-Path $root 'differential.json') -Raw | ConvertFrom-Json);production_integrated=$false;synthetic_only=$true;build_slot='exclusive FileShare.None'}
    $record | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $root 'verification.json') -Encoding utf8
    $record | ConvertTo-Json -Depth 10
}finally{
    $env:PATH=$oldPath;$env:LIB=$oldLib
    if($handle){$handle.Dispose();Write-Output ('BUILD SLOT '+$slot+' released; verifier PID '+$PID)}
}
