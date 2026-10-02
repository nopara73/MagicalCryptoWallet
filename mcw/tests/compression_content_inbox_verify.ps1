param([Parameter(Mandatory=$true)][string]$ReviewRoot,
    [string]$SharedRoot='C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',
    [int]$WaitForSlotSeconds=0)
$ErrorActionPreference='Stop'
$repo=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$review=[IO.Path]::GetFullPath($ReviewRoot)
$artifacts=[IO.Path]::GetFullPath((Join-Path $repo '.artifacts'))+[IO.Path]::DirectorySeparatorChar
if(-not $review.StartsWith($artifacts,[StringComparison]::OrdinalIgnoreCase)){throw 'Review must be in the owned artifacts directory'}
$manifest=Get-Content -LiteralPath (Join-Path $review 'patch-manifest.json') -Raw | ConvertFrom-Json
$root=Join-Path $review ('verification-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root | Out-Null
$hashes=[ordered]@{}
function Assert-ReviewHashes {
    foreach($entry in $manifest.source_hashes.PSObject.Properties){
        if((Get-FileHash -LiteralPath (Join-Path $review ('source/'+$entry.Name)) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.Value){throw ('Original snapshot changed: '+$entry.Name)}
    }
    foreach($entry in $manifest.review_hashes.PSObject.Properties){
        if((Get-FileHash -LiteralPath (Join-Path $review ('review/'+$entry.Name)) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.Value){throw ('Review snapshot changed: '+$entry.Name)}
    }
}
Assert-ReviewHashes
$owned=@('mcw/src/compression.rs','mcw/tests/compression_fixtures/content_inbox_tests.rs','mcw/tests/compression_content_inbox_verify.ps1','mcw/tests/compression_content_host_patch.py')
$owned+=Get-ChildItem -LiteralPath (Join-Path $repo 'mcw/src/content_service') -File -Recurse | ForEach-Object { [IO.Path]::GetRelativePath($repo,$_.FullName).Replace('\','/') }
foreach($name in $owned){
    $source=Join-Path $repo $name
    $destination=Join-Path $root ('owned/'+$name)
    New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($destination)) -Force | Out-Null
    Copy-Item -LiteralPath $source -Destination $destination
    $hashes[$name]=(Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
    if((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant() -ne $hashes[$name]){throw ('Owned source changed during snapshot: '+$name)}
}
$wrapper=@'
#![forbid(unsafe_code)]
#[path="COMPRESSION"] pub mod compression;
#[path="CONTENT"] pub mod content_service;
pub mod qr;
pub mod bridge;
mod inbox;
#[cfg(test)]
#[path="TESTS"] mod content_inbox_tests;
'@
foreach($item in @(@('COMPRESSION','owned/mcw/src/compression.rs'),@('CONTENT','owned/mcw/src/content_service/mod.rs'),@('TESTS','owned/mcw/tests/compression_fixtures/content_inbox_tests.rs'))){
    $wrapper=$wrapper.Replace($item[0],(Join-Path $root $item[1]).Replace('\','/'))
}
$compiledHost=[ordered]@{}
foreach($item in @(@('mcw/src/qr.rs','qr.rs'),@('mcw/src/qr/tables.rs','qr/tables.rs'),@('mcw/src/bridge.rs','bridge.rs'),@('mcw/src/app/inbox.rs','inbox.rs'))){
    $target=Join-Path $root ('src/'+$item[1])
    New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($target)) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $review ('review/'+$item[0])) -Destination $target
    $expected=$manifest.review_hashes.($item[0])
    if((Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected){throw ('Compiled host copy differs: '+$item[0])}
    $compiledHost[$item[1]]=$expected
}
$source=Join-Path $root 'src/lib.rs'
[IO.File]::WriteAllText($source,$wrapper)
$rustBin=Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin'
$handle=$null
$oldPath=$env:PATH
$oldLib=$env:LIB
try {
    $waiting=[Diagnostics.Stopwatch]::StartNew()
    do {
        if((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory -lt 2097152){throw 'Build deferred: RAM below 2 GiB'}
        foreach($slot in 1,2){
            try {$handle=[IO.File]::Open((Join-Path $SharedRoot ('.artifacts/mcw-coordination/build-slot-'+$slot+'.lock')),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None);break}
            catch [IO.IOException] { }
        }
        if($handle -or $waiting.Elapsed.TotalSeconds -ge $WaitForSlotSeconds){break}
        Write-Output ('Existing synchronized content/inbox proof waiting; no build slot held; PID '+$PID)
        Start-Sleep -Seconds 30
    }while($true)
    if(-not $handle){throw 'Build deferred: slots occupied'}
    Write-Output ('BUILD SLOT '+$slot+'; verifier PID '+$PID+'; existing synchronized content/inbox cancellation proof')
    $rustc=Join-Path $rustBin 'rustc.exe'
    $compiler=& $rustc --version
    if($LASTEXITCODE -ne 0 -or $compiler -notlike 'rustc 1.99.0 *'){throw 'Rust 1.99.0 required'}
    $linker=Get-ChildItem -Path 'C:\Program Files\Microsoft Visual Studio\*\*\VC\Tools\MSVC\*\bin\Hostx64\x64\link.exe' | Sort-Object FullName -Descending | Select-Object -First 1
    $msvc=[IO.Path]::GetFullPath((Join-Path $linker.Directory.FullName '../../..'))
    $sdk=Get-ChildItem -LiteralPath 'C:\Program Files (x86)\Windows Kits\10\Lib' -Directory | Sort-Object Name -Descending | Select-Object -First 1
    $env:PATH=$linker.Directory.FullName+';'+$rustBin+';'+$oldPath
    $env:LIB=(Join-Path $msvc 'lib/onecore/x64')+';'+(Join-Path $sdk.FullName 'ucrt/x64')+';'+(Join-Path $sdk.FullName 'um/x64')
    $tests=[ordered]@{}
    foreach($profile in @(@('debug','0'),@('optimized','2'))){
        $exe=Join-Path $root ($profile[0]+'-inbox-tests.exe')
        & $rustc --edition=2024 --test -D warnings -C codegen-units=1 -C overflow-checks=yes -C target-feature=+crt-static -C ('opt-level='+$profile[1]) $source -o $exe
        if($LASTEXITCODE -ne 0){throw ($profile[0]+' inbox component compilation failed')}
        $log=& $exe --test-threads=1 2>&1
        $testExit=$LASTEXITCODE
        $log | Set-Content -LiteralPath (Join-Path $root ($profile[0]+'.log')) -Encoding utf8
        $log | Write-Output
        if($testExit -ne 0){throw ($profile[0]+' synchronized inbox component verification failed')}
        if(@($log | Select-String -Pattern '^test content_inbox_tests::.* \.\.\. ok$').Count -ne 7){throw 'Seven synchronized content/inbox cases required'}
        $tests[$profile[0]]=@{passed=$true;result=(@($log | Select-String -Pattern '^test result:') -join ' ');synchronized_content_cases=7}
    }
    Assert-ReviewHashes
    foreach($name in $compiledHost.Keys){
        if((Get-FileHash -LiteralPath (Join-Path $root ('src/'+$name)) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $compiledHost[$name]){throw ('Compiled host snapshot changed: '+$name)}
    }
    foreach($name in $owned){
        if((Get-FileHash -LiteralPath (Join-Path $repo $name) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $hashes[$name]){throw ('Owned source changed during verification: '+$name)}
    }
    $record=@{compiler=$compiler;edition=2024;platform='x86_64-pc-windows-msvc';source_hashes=$hashes;host_review=$manifest;compiled_host_hashes=$compiledHost;tests=$tests;
        actual_application_host=$false;production_integrated=$false;test_local_shared_hook=$true;synthetic_only=$true;loopback_only=$true;
        synchronized_inner_work_checkpoint=20;partial_output_required=$true;private_failure_bytes=22;
        unsatisfied_gates=@('Real host owner interruption hook and content registration','Real selected factory activation','In-flight native cancellation/EOF/queue proof through incorporated master host');
        snapshot_root=$root;build_slot='exclusive FileShare.None';verifier_pid=$PID}
    $record | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath (Join-Path $root 'verification.json') -Encoding utf8
    Write-Output ('SYNCHRONIZED COMPONENT EVIDENCE '+(Join-Path $root 'verification.json'))
}finally{
    $env:PATH=$oldPath;$env:LIB=$oldLib
    if($handle){$handle.Dispose();Write-Output ('BUILD SLOT '+$slot+' released; verifier PID '+$PID)}
}
