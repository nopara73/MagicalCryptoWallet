param([string]$SharedRoot='C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',[switch]$Format,[string]$OutputDirectory)
$ErrorActionPreference='Stop'
$componentRoot=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$rustBin=Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin'
$rustc=Join-Path $rustBin 'rustc.exe'
$version=(& $rustc --version)-join ''
if($LASTEXITCODE -or $version -notmatch '^rustc 1\.99\.0 '){throw 'Rust 1.99.0 required'}
if(([double](Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory/1MB)-lt 2){throw 'Verification deferred: less than 2 GiB free memory'}
$slotHandle=$null
foreach($slot in 1..2){try{$slotHandle=[IO.File]::Open((Join-Path $SharedRoot ".artifacts/mcw-coordination/build-slot-$slot.lock"),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None);break}catch [IO.IOException]{}}
if(-not $slotHandle){throw 'Verification deferred: both build slots occupied'}
Write-Output "Build slot $slot; verifier PID $PID; existing bounded Markdown assignment"
try{
    if(-not $env:VCToolsInstallDir){
        $vswhere=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
        $vsRoot=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if(-not $vsRoot){throw 'Installed linker not available'}
        & (Join-Path $vsRoot 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation *> $null
    }
    $output=if($OutputDirectory){[IO.Path]::GetFullPath($OutputDirectory)}else{Join-Path $componentRoot ('.artifacts/native-ui-markdown-verification/runs/'+[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ'))}
    [IO.Directory]::CreateDirectory($output)|Out-Null
    $source=Join-Path $PSScriptRoot 'native_ui_markdown_conformance.rs'
    foreach($formatSource in $source,(Join-Path $PSScriptRoot 'native_ui_markdown_probe.rs')){
        if($Format){& (Join-Path $rustBin 'rustfmt.exe') --edition 2024 $formatSource; if($LASTEXITCODE){throw 'Formatting failed'}}
        & (Join-Path $rustBin 'rustfmt.exe') --edition 2024 --check $formatSource
        if($LASTEXITCODE){throw 'Formatting check failed'}
    }
    $inputs=Join-Path $componentRoot '.artifacts/native-ui-markdown-inputs'
    if(-not (Test-Path -LiteralPath $inputs)){
        & python (Join-Path $PSScriptRoot 'native_ui_markdown_inventory.py')
        if($LASTEXITCODE){throw 'Input inventory failed'}
    }
    $managedProject=Join-Path $componentRoot 'Contrib/McwMigration/NativeUiVerification/NativeUiVerification.csproj'
    & dotnet restore $managedProject --disable-parallel --locked-mode -p:Configuration=Release -m:1
    if($LASTEXITCODE){throw 'Managed presentation restore failed'}
    & python (Join-Path $PSScriptRoot 'native_ui_markdown_bindings.py') --write (Join-Path $output 'compiler-inputs-before.json')
    if($LASTEXITCODE){throw 'Compiler input binding failed'}
    & (Join-Path $rustBin 'clippy-driver.exe') --edition=2024 --crate-type=lib -Dwarnings -C codegen-units=1 (Join-Path $componentRoot 'mcw/src/markdown/mod.rs') -o (Join-Path $output 'markdown-lint.rlib')
    if($LASTEXITCODE){throw 'Shipping Markdown lint failed'}
    & $rustc --edition=2024 --test -Dwarnings -C codegen-units=1 $source -o (Join-Path $output 'native_ui_markdown_conformance.exe')
    if($LASTEXITCODE){throw 'Markdown test compilation failed'}
    & (Join-Path $output 'native_ui_markdown_conformance.exe') --test-threads=1 2>&1 | Tee-Object -FilePath (Join-Path $output 'conformance-results.txt')
    if($LASTEXITCODE){throw 'Markdown tests failed'}
    & $rustc --edition=2024 -Dwarnings -C codegen-units=1 (Join-Path $PSScriptRoot 'native_ui_markdown_probe.rs') -o (Join-Path $output 'native_ui_markdown_probe.exe')
    if($LASTEXITCODE){throw 'Markdown wire probe compilation failed'}
    $wireOutput=Join-Path $output 'wire'
    [IO.Directory]::CreateDirectory($wireOutput)|Out-Null
    foreach($inputFile in Get-ChildItem -LiteralPath $inputs -Filter '*.md'){
        & (Join-Path $output 'native_ui_markdown_probe.exe') $inputFile.FullName (Join-Path $wireOutput ($inputFile.BaseName+'.bin'))
        if($LASTEXITCODE){throw ('Wire probe failed: '+$inputFile.Name)}
    }
    & dotnet build $managedProject -c Release --no-restore -m:1
    if($LASTEXITCODE){throw 'Managed presentation compilation failed'}
    & dotnet run --project $managedProject -c Release --no-build -- (Join-Path $output 'render') $wireOutput
    if($LASTEXITCODE){throw 'Managed presentation verification failed'}
    & python (Join-Path $PSScriptRoot 'native_ui_markdown_legacy_compare.py') --output $output
    if($LASTEXITCODE){throw 'Legacy presentation comparison failed'}
    & python (Join-Path $PSScriptRoot 'native_ui_markdown_bindings.py') --check (Join-Path $output 'compiler-inputs-before.json') --report (Join-Path $output 'compiler-inputs-after.json')
    if($LASTEXITCODE){throw 'Compiler inputs changed during verification'}
    [ordered]@{scope='bounded Markdown parse service; no full UI rewrite';rust=$version;native_target='x86_64-pc-windows-msvc';test_log='conformance-results.txt';compiler_inputs_before='compiler-inputs-before.json';compiler_inputs_after='compiler-inputs-after.json';completed_utc=[DateTime]::UtcNow.ToString('O');production_integrated=$false;dependency_removed=$false;native_in_progress_cancellation_verified=$false}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $output 'verification.json') -Encoding utf8
    Write-Output "Candidate verification evidence: $output"
}finally{$slotHandle.Dispose()}
