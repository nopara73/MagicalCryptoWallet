param([string]$SharedRoot='C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet')
$ErrorActionPreference='Stop'
$taskRoot=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../..'))
$evidence=Join-Path $taskRoot '.artifacts/safe-file-evidence'
[IO.Directory]::CreateDirectory($evidence)|Out-Null
$toolBin=Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin'
$native=Join-Path $taskRoot 'mcw/src/platform/safe_file/mod.rs'
if(-not(Test-Path -LiteralPath $native)){throw 'Assigned native SafeFile leaf is missing'}
$module=Join-Path $taskRoot 'mcw/src/safe_file_service/mod.rs'
$tests=Join-Path $PSScriptRoot 'safe_file_tests.rs'
$sources=@(Get-ChildItem -LiteralPath (Join-Path $taskRoot 'mcw/src') -Recurse -File -Filter '*.rs')+@(Get-ChildItem -LiteralPath $PSScriptRoot -File)+@(Get-Item -LiteralPath (Join-Path $taskRoot 'MagicalCryptoWallet/Io/SafeFile.cs'),(Join-Path $taskRoot 'MagicalCryptoWallet/Mcw/Storage/McwSafeFile.cs'),(Join-Path $taskRoot 'MagicalCryptoWallet/Mcw/IMcwApplicationServices.cs'),(Join-Path $taskRoot 'MagicalCryptoWallet.Client/Application/ManagedApplicationHost.cs'))
$hashes=[ordered]@{}
foreach($source in $sources){$hashes[$source.FullName]=(Get-FileHash -LiteralPath $source.FullName -Algorithm SHA256).Hash}
$slot=$null
try {
    if((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory -lt 2097152){Write-Output 'BUILD_DEFERRED_MEMORY';exit 75}
    foreach($n in 1,2){try{$slot=[IO.File]::Open((Join-Path $SharedRoot ".artifacts/mcw-coordination/build-slot-$n.lock"),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None);break} catch [IO.IOException] {}}
    if(-not $slot){Write-Output 'BUILD_SLOTS_BUSY';exit 75}
    $vswhere=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $vsRoot=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    & (Join-Path $vsRoot 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation | Out-Null
    $rustc=Join-Path $toolBin 'rustc.exe';$version=& $rustc --version
    if($version -notlike 'rustc 1.99.0 *'){throw 'Rust 1.99.0 required'}
    $library='extern crate self as mcw;'+"`n"+'#[path="'+$module.Replace('\','/')+'"] pub mod safe_file_service;'+"`n"+'pub mod platform { #[path="'+$native.Replace('\','/')+'"] pub mod safe_file; }'+"`n"
    $harness=Join-Path $evidence 'tests.rs';[IO.File]::WriteAllText($harness,$library+'#[path="'+$tests.Replace('\','/')+'"] mod safe_file_tests;',[Text.UTF8Encoding]::new($false))
    & (Join-Path $toolBin 'rustfmt.exe') --edition 2024 --check $module $tests (Join-Path $PSScriptRoot 'safe_file_driver.rs') $native
    if($LASTEXITCODE){throw 'Safe-file formatting failed'}
    & (Join-Path $toolBin 'clippy-driver.exe') --edition=2024 --test -D warnings -W clippy::all -D clippy::undocumented_unsafe_blocks -C overflow-checks=yes --emit=metadata $harness -o (Join-Path $evidence 'clippy.rmeta')
    if($LASTEXITCODE){throw 'Safe-file clippy failed'}
    $binary=Join-Path $evidence 'safe-file-tests.exe'
    & $rustc --edition=2024 --test -D warnings -C overflow-checks=yes -C target-feature=+crt-static -C linker=link.exe $harness -o $binary
    if($LASTEXITCODE){throw 'Safe-file tests compile failed'}
    & $binary --test-threads=1 2>&1 | Tee-Object -FilePath (Join-Path $evidence 'rust-tests.txt')
    if($LASTEXITCODE){throw 'Safe-file tests failed'}
    $driverHarness=Join-Path $evidence 'driver.rs';[IO.File]::WriteAllText($driverHarness,$library+[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'safe_file_driver.rs')),[Text.UTF8Encoding]::new($false))
    $driver=Join-Path $evidence 'safe-file-driver.exe'
    & $rustc --edition=2024 -D warnings -C overflow-checks=yes -C target-feature=+crt-static -C linker=link.exe $driverHarness -o $driver
    if($LASTEXITCODE){throw 'Safe-file driver compile failed'}
    # Generate the exact proposed caller change in an ignored copy. Production
    # callers can remain unchanged until the integrator enables the full host.
    $managedCandidate=Join-Path $evidence 'cutover/SafeFile.cs'
    & 'C:/Python314/python.exe' (Join-Path $PSScriptRoot 'make_registration_patch.py') --repo $taskRoot --managed-candidate $managedCandidate
    if($LASTEXITCODE){throw 'SafeFile caller changed; manual reconciliation required'}
    if(-not([IO.File]::ReadAllText($managedCandidate).Contains('McwSafeFile.WriteAllText'))){throw 'Proposed SafeFile cutover is missing; legacy-against-legacy parity is not evidence'}
    $hashes[$managedCandidate]=(Get-FileHash -LiteralPath $managedCandidate -Algorithm SHA256).Hash
    $managedProject=Join-Path $PSScriptRoot 'SafeFileChecks.csproj'
    $managedOutput=(Join-Path $evidence 'managed-bin')+[IO.Path]::DirectorySeparatorChar
    $managedIntermediate=(Join-Path $evidence 'managed-obj')+[IO.Path]::DirectorySeparatorChar
    & dotnet build $managedProject -p:ImportDirectoryBuildProps=false -p:ImportDirectoryBuildTargets=false -p:SafeFileCandidatePath=$managedCandidate -p:BaseOutputPath=$managedOutput -p:BaseIntermediateOutputPath=$managedIntermediate -m:1 --nologo
    if($LASTEXITCODE){throw 'Safe-file managed differential harness compile failed'}
    $run=Join-Path $evidence ('run-'+[DateTimeOffset]::UtcNow.ToString('yyyyMMddTHHmmssfffZ'))
    & dotnet (Join-Path $managedOutput 'Debug/net10.0/SafeFileChecks.dll') $driver (Join-Path $run 'managed')
    if($LASTEXITCODE){throw 'Safe-file managed differential cases failed'}
    & 'C:/Python314/python.exe' (Join-Path $PSScriptRoot 'safe_file_crash.py') --driver $driver --output (Join-Path $run 'crash')
    if($LASTEXITCODE){throw 'Safe-file process interruption cases failed'}
    # Compile an ignored overlay of the exact proposed shared registration. The
    # application, framing and managed host sources are real published source;
    # this is development-only evidence, not a production cutover/import audit.
    $overlay=Join-Path $evidence 'host-overlay'
    & 'C:/Python314/python.exe' (Join-Path $PSScriptRoot 'make_registration_patch.py') --repo $taskRoot --overlay $overlay
    if($LASTEXITCODE){throw 'Shared host registration changed; manual reconciliation required'}
    & (Join-Path $toolBin 'rustfmt.exe') --edition 2024 --check (Join-Path $overlay 'app.rs') (Join-Path $overlay 'lib.rs') (Join-Path $overlay 'platform.rs')
    if($LASTEXITCODE){throw 'Proposed shared registration formatting failed'}
    $hostDirectory=Join-Path $run 'host'
    [IO.Directory]::CreateDirectory($hostDirectory)|Out-Null
    $hostLibrary=Join-Path $hostDirectory 'libmcw.rlib'
    $hostTests=Join-Path $hostDirectory 'host-tests.exe'
    & $rustc --edition=2024 --test --crate-name mcw -D warnings -C overflow-checks=yes -C target-feature=+crt-static -C linker=link.exe (Join-Path $overlay 'lib.rs') -o $hostTests
    if($LASTEXITCODE){throw 'Development host dispatch tests compile failed'}
    & $hostTests safe_file_host_registration_tests --test-threads=1 2>&1|Tee-Object -FilePath (Join-Path $run 'host-dispatch-tests.txt')
    if($LASTEXITCODE){throw 'Development host dispatch tests failed'}
    & $rustc --edition=2024 --crate-type=rlib --crate-name mcw -D warnings -C overflow-checks=yes -C target-feature=+crt-static (Join-Path $overlay 'lib.rs') -o $hostLibrary
    if($LASTEXITCODE){throw 'Development application-host overlay compile failed'}
    $hostBinary=Join-Path $hostDirectory 'mcw.exe'
    & $rustc --edition=2024 --extern "mcw=$hostLibrary" -D warnings -C overflow-checks=yes -C target-feature=+crt-static -C linker=link.exe (Join-Path $taskRoot 'mcw/src/main.rs') -o $hostBinary
    if($LASTEXITCODE){throw 'Development application-host executable compile failed'}
    $managedBinary=Join-Path $managedOutput 'Debug/net10.0'
    foreach($file in 'SafeFileChecks.dll','SafeFileChecks.deps.json','SafeFileChecks.runtimeconfig.json'){Copy-Item -LiteralPath (Join-Path $managedBinary $file) -Destination (Join-Path $hostDirectory $file)}
    Copy-Item -LiteralPath (Join-Path $managedBinary 'SafeFileChecks.exe') -Destination (Join-Path $hostDirectory 'magicalcryptowallet.exe')
    $hostStart=[Diagnostics.ProcessStartInfo]::new($hostBinary)
    $hostStart.UseShellExecute=$false;$hostStart.CreateNoWindow=$true;$hostStart.WindowStyle=[Diagnostics.ProcessWindowStyle]::Hidden
    $hostStart.RedirectStandardError=$true
    $hostStart.Environment.Remove('MCW_HOSTED')|Out-Null;$hostStart.Environment.Remove('MCW_HOST_PATH')|Out-Null
    $hostStart.ArgumentList.Add('gui');$hostStart.ArgumentList.Add('--host-checks');$hostStart.ArgumentList.Add((Join-Path $run 'application-bridge'))
    $hostProcess=[Diagnostics.Process]::Start($hostStart)
    try {
        $hostOutput=$hostProcess.StandardError.ReadToEndAsync()
        if(-not $hostProcess.WaitForExit(60000)){$hostProcess.Kill($true);throw 'Owned synthetic application-host case timed out'}
        [IO.File]::WriteAllText((Join-Path $run 'application-host-output.txt'),$hostOutput.GetAwaiter().GetResult())
        if($hostProcess.ExitCode){throw 'Actual application-bridge differential cases failed'}
        $hostProof=Get-Content -LiteralPath (Join-Path $run 'application-bridge/managed-reference.json') -Raw|ConvertFrom-Json
        if(-not $hostProof.result.passed -or -not $hostProof.result.actual_application_bridge){throw 'Application-bridge completion was not verified'}
        $hostProof.result|ConvertTo-Json -Compress|Write-Output
    }finally{$hostProcess.Dispose()}
    foreach($source in $sources){if($hashes[$source.FullName] -ne (Get-FileHash -LiteralPath $source.FullName -Algorithm SHA256).Hash){throw 'Sources changed during verification'}}
    if($hashes[$managedCandidate] -ne (Get-FileHash -LiteralPath $managedCandidate -Algorithm SHA256).Hash){throw 'Proposed caller changed during verification'}
    $hashes|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $evidence 'compiled-source-hashes.json') -Encoding utf8NoBOM
    [ordered]@{rust=$version;windows='native_tests_and_managed_differential_passed';clippy='passed';managed_case_evidence=Join-Path $run 'managed/managed-reference.json';application_bridge_evidence=Join-Path $run 'application-bridge/managed-reference.json';interruption_evidence=Join-Path $run 'crash/crash.json';other_four_targets='unverified';shipping_import_audit='unverified';physical_power_loss='unverified';scope='safe-file only';sources=$hashes}|ConvertTo-Json -Depth 4|Set-Content -LiteralPath (Join-Path $evidence 'verification.json') -Encoding utf8NoBOM
}finally{if($slot){$slot.Dispose()}}
