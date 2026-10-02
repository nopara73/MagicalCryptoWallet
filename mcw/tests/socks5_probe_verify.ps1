param(
    [string]$SharedRoot = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',
    [string]$VerificationBase,
    [switch]$VerifyPrivacyControl
)
$ErrorActionPreference = 'Stop'
$taskRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$outputRoot = Join-Path $taskRoot '.artifacts/socks5-probe'
[IO.Directory]::CreateDirectory($outputRoot) | Out-Null
$managerSource = [IO.File]::ReadAllText((Join-Path $taskRoot 'MagicalCryptoWallet/Tor/TorProcessManager.cs'))
$readerSource = [IO.File]::ReadAllText((Join-Path $taskRoot 'MagicalCryptoWallet/Tor/Control/TorControlReplyReader.cs'))
if (-not $managerSource.Contains('readReply') -or -not $readerSource.Contains('McwTorControlCodec.ReadReplyAsync')) {
    throw 'Apply the published privacy caller/host patches before verifying the explicit wallet readiness fixture.'
}
$slotHandle = $null
$freeGiB = [double](Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB
if ($freeGiB -lt 2) { throw 'Verification deferred: less than 2 GiB free.' }
foreach ($slot in 1..2) {
    try { $slotHandle = [IO.File]::Open((Join-Path $SharedRoot ".artifacts/mcw-coordination/build-slot-$slot.lock"),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None); break }
    catch [IO.IOException] { }
}
if (-not $slotHandle) { throw 'Verification deferred: shared build slots busy.' }
$variables = @('PATH','CARGO_HOME','RUSTUP_HOME','CARGO_TARGET_DIR','CARGO_BUILD_JOBS')
$previous = @{}
foreach ($variable in $variables) { $previous[$variable] = [Environment]::GetEnvironmentVariable($variable,'Process') }
try {
    $toolsRoot = Join-Path $SharedRoot '.artifacts/mcw-tools'
    $rustBin = Join-Path $toolsRoot 'rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin'
    $env:PATH = $rustBin + ';' + $env:PATH
    $env:CARGO_HOME = Join-Path $toolsRoot 'cargo'
    $env:RUSTUP_HOME = Join-Path $toolsRoot 'rustup'
    $env:CARGO_TARGET_DIR = Join-Path $outputRoot 'target'
    $env:CARGO_BUILD_JOBS = '1'
    & (Join-Path $taskRoot 'Contrib/Mcw/build-windows.ps1') -Cargo (Join-Path $rustBin 'cargo.exe') 2>&1 | Tee-Object -FilePath (Join-Path $outputRoot 'native-build.txt')
    if ($LASTEXITCODE) { throw 'Actual host build failed.' }
    $binary = Join-Path $env:CARGO_TARGET_DIR 'x86_64-pc-windows-msvc/release/mcw.exe'
    if (-not (Test-Path -LiteralPath $binary)) { throw 'Native host binary is missing.' }
    & (Join-Path $rustBin 'rustfmt.exe') --edition 2024 --check (Join-Path $taskRoot 'mcw/src/socks5.rs') (Join-Path $taskRoot 'mcw/tests/socks5_probe_service.rs')
    if ($LASTEXITCODE) { throw 'Formatting check failed.' }
    $harness = Join-Path $outputRoot 'lib.rs'
    [IO.File]::WriteAllText($harness,'#[path="../../mcw/src/socks5.rs"] pub mod socks5;' + "`n")
    & (Join-Path $rustBin 'clippy-driver.exe') --edition=2024 --crate-name socks5_probe --crate-type=lib --emit=metadata -Dwarnings -Dclippy::all $harness -o (Join-Path $outputRoot 'probe.rmeta')
    if ($LASTEXITCODE) { throw 'Strict module lint failed.' }
    foreach ($name in @('wire','transport','probe_service')) {
        $testBinary = Join-Path $outputRoot "socks5_$name.exe"
        & (Join-Path $rustBin 'rustc.exe') --edition=2024 --test -Dwarnings (Join-Path $PSScriptRoot "socks5_$name.rs") -o $testBinary
        if ($LASTEXITCODE) { throw "$name test compilation failed." }
        & $testBinary --test-threads=1 2>&1 | Tee-Object -FilePath (Join-Path $outputRoot "$name-tests.txt")
        if ($LASTEXITCODE) { throw "$name tests failed." }
    }
    if ([double](Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB -lt 2) { throw 'Managed verification deferred: less than 2 GiB free.' }
    $managedRoot = Join-Path $outputRoot 'ManagedProbe'
    [IO.Directory]::CreateDirectory($managedRoot) | Out-Null
    $project = @'
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup><OutputType>Exe</OutputType><AssemblyName>SocksProbe</AssemblyName><DisableImplicitNamespaceImports>true</DisableImplicitNamespaceImports></PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../../../MagicalCryptoWallet/MagicalCryptoWallet.csproj" />
    <Compile Include="../../../MagicalCryptoWallet.Client/Application/ManagedApplicationHost.cs" Link="ManagedApplicationHost.cs" />
    <Compile Include="../../../mcw/tests/socks5_probe_caller.cs" Link="Program.cs" />
  </ItemGroup>
</Project>
'@
    [IO.File]::WriteAllText((Join-Path $managedRoot 'SocksProbe.csproj'),$project)
    & dotnet build (Join-Path $managedRoot 'SocksProbe.csproj') -c Release -m:1 -p:UseSharedCompilation=false -p:NuGetAudit=false -p:CopyToOutputDirectory=Never 2>&1 | Tee-Object -FilePath (Join-Path $outputRoot 'managed-build.txt')
    if ($LASTEXITCODE) { throw 'Real managed caller compilation failed.' }
    $runRoot = Join-Path $outputRoot 'runtime'
    [IO.Directory]::CreateDirectory($runRoot) | Out-Null
    Copy-Item -Path (Join-Path $managedRoot 'bin/Release/net10.0/*') -Destination $runRoot -Recurse -Force
    Copy-Item -LiteralPath $binary -Destination (Join-Path $runRoot 'mcw.exe') -Force
    Copy-Item -LiteralPath (Join-Path $runRoot 'SocksProbe.exe') -Destination (Join-Path $runRoot 'magicalcryptowallet.exe') -Force
    $report = Join-Path $runRoot 'results.json'
    $errorLog = Join-Path $outputRoot 'caller-stderr.txt'
    $outputLog = Join-Path $outputRoot 'caller-stdout.txt'
    $runArguments = @('gui','socks-probe-tests',('"' + $report + '"'))
    $child = Start-Process -FilePath (Join-Path $runRoot 'mcw.exe') -ArgumentList $runArguments -WindowStyle Hidden -PassThru -RedirectStandardError $errorLog -RedirectStandardOutput $outputLog
    if (-not $child.WaitForExit(60000)) { throw 'Synthetic probe test exceeded 60 seconds; inspect the owned test process.' }
    if ($child.ExitCode -ne 0) { Get-Content -LiteralPath $errorLog; throw ('Synthetic production caller failed: ' + $child.ExitCode) }
    $result = Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
    if ($result.checks -ne 31 -or $result.adapterChecks -ne 13 -or $result.productionRustBoundaryChecks -ne 18 -or
        -not $result.noTorLaunched -or -not $result.noWalletData -or $result.exactDeadlineTimingProven) {
        throw 'Grouped production caller evidence is incomplete.'
    }
    $diagnostics = [IO.File]::ReadAllText($errorLog) + [IO.File]::ReadAllText($outputLog)
    if ($diagnostics -match '127\.0\.0\.1|example\.invalid|192\.0\.2\.1|synthetic-isolation') { throw 'Sensitive endpoint/payload appeared in diagnostics.' }
    if ($VerifyPrivacyControl) {
        if ([double](Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB -lt 2) { throw 'Control verification deferred: less than 2 GiB free.' }
        $controlProject = Join-Path $taskRoot 'Contrib/McwMigration/PrivacyControlProbe/PrivacyControlProbe.csproj'
        & dotnet build $controlProject -c Release -m:1 -p:UseSharedCompilation=false -p:NuGetAudit=false -p:CopyToOutputDirectory=Never 2>&1 | Tee-Object -FilePath (Join-Path $outputRoot 'privacy-managed-build.txt')
        if ($LASTEXITCODE) { throw 'Unchanged privacy control fixture build failed.' }
        $controlRuntime = Join-Path $outputRoot 'privacy-runtime'
        [IO.Directory]::CreateDirectory($controlRuntime) | Out-Null
        Copy-Item -Path (Join-Path $taskRoot 'Contrib/McwMigration/PrivacyControlProbe/bin/Release/net10.0/*') -Destination $controlRuntime -Recurse -Force
        Copy-Item -LiteralPath $binary -Destination (Join-Path $controlRuntime 'mcw.exe') -Force
        foreach ($name in @('magicalcryptowallet')) {
            Copy-Item -LiteralPath (Join-Path $controlRuntime 'PrivacyControlProbe.exe') -Destination (Join-Path $controlRuntime ($name + '.exe')) -Force
        }
        $controlResults = @{}
        foreach ($mode in @('gui')) {
            $controlReport = Join-Path $controlRuntime ($mode + '.json')
            $controlError = Join-Path $outputRoot ('privacy-' + $mode + '-stderr.txt')
            $controlOutput = Join-Path $outputRoot ('privacy-' + $mode + '-stdout.txt')
            $fixtures = Join-Path $taskRoot 'mcw/tests/privacy_control_fixtures/replies.tsv'
            $controlArguments = @($mode,('"' + $controlReport + '"'),('"' + $fixtures + '"'))
            $controlChild = Start-Process -FilePath (Join-Path $controlRuntime 'mcw.exe') -ArgumentList $controlArguments -WindowStyle Hidden -PassThru -RedirectStandardError $controlError -RedirectStandardOutput $controlOutput
            if (-not $controlChild.WaitForExit(60000)) { throw 'Synthetic control fixture exceeded 60 seconds; inspect the owned test process.' }
            if ($controlChild.ExitCode -ne 0) { Get-Content -LiteralPath $controlError; throw ('Combined control fixture failed: ' + $controlChild.ExitCode) }
            $controlResult = Get-Content -LiteralPath $controlReport -Raw | ConvertFrom-Json
            if ($controlResult.assertions -ne 102 -or -not $controlResult.productionRustBridge -or -not $controlResult.coordinatorExcluded) {
                throw 'Combined privacy proof is incomplete.'
            }
            if ([IO.File]::ReadAllText($controlOutput).Length) { throw 'Control payload unexpectedly appeared on host stdout.' }
            $controlResults[$mode] = $controlResult
        }
        $result | Add-Member -NotePropertyName combined_privacy_control -NotePropertyValue $controlResults
    }
    if ([double](Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB -lt 2) { throw 'Coordinator verification deferred: less than 2 GiB free.' }
    $coordinatorRoot = Join-Path $outputRoot 'CoordinatorProbe'
    [IO.Directory]::CreateDirectory($coordinatorRoot) | Out-Null
    $coordinatorProject = @'
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup><OutputType>Exe</OutputType><AssemblyName>CoordinatorProbe</AssemblyName><DisableImplicitNamespaceImports>true</DisableImplicitNamespaceImports></PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../../../MagicalCryptoWallet.Coordinator/MagicalCryptoWallet.Coordinator.csproj" />
    <Compile Include="../../../mcw/tests/socks5_probe_coordinator_test.cs" Link="Program.cs" />
  </ItemGroup>
</Project>
'@
    [IO.File]::WriteAllText((Join-Path $coordinatorRoot 'CoordinatorProbe.csproj'),$coordinatorProject)
    & dotnet build (Join-Path $coordinatorRoot 'CoordinatorProbe.csproj') -c Release -m:1 -p:UseSharedCompilation=false -p:NuGetAudit=false -p:CopyToOutputDirectory=Never 2>&1 | Tee-Object -FilePath (Join-Path $outputRoot 'coordinator-build.txt')
    if ($LASTEXITCODE) { throw 'Actual external coordinator build failed.' }
    $coordinatorReport = Join-Path $coordinatorRoot 'results.json'
    $coordinatorError = Join-Path $outputRoot 'coordinator-stderr.txt'
    $coordinatorOutput = Join-Path $outputRoot 'coordinator-stdout.txt'
    $coordinatorDll = Join-Path $coordinatorRoot 'bin/Release/net10.0/CoordinatorProbe.dll'
    $coordinatorArguments = @(('"' + $coordinatorDll + '"'),('"' + $coordinatorReport + '"'))
    $coordinatorChild = Start-Process -FilePath (Get-Command dotnet).Source -ArgumentList $coordinatorArguments -WindowStyle Hidden -PassThru -RedirectStandardError $coordinatorError -RedirectStandardOutput $coordinatorOutput
    if (-not $coordinatorChild.WaitForExit(60000)) { throw 'Synthetic unhosted coordinator fixture exceeded 60 seconds; inspect the owned test process.' }
    if ($coordinatorChild.ExitCode -ne 0) { Get-Content -LiteralPath $coordinatorError; throw ('Unhosted coordinator fixture failed: ' + $coordinatorChild.ExitCode) }
    $coordinatorResult = Get-Content -LiteralPath $coordinatorReport -Raw | ConvertFrom-Json
    if ($coordinatorResult.checks -ne 15 -or -not $coordinatorResult.applicationHostBindingAbsent -or
        -not $coordinatorResult.walletRequiresRust -or -not $coordinatorResult.explicitCoordinatorReadinessAndParser -or
        -not $coordinatorResult.noTorLaunched -or -not $coordinatorResult.noWalletData) { throw 'External coordinator role proof is incomplete.' }
    $coordinatorDiagnostics = [IO.File]::ReadAllText($coordinatorError) + [IO.File]::ReadAllText($coordinatorOutput)
    if ($coordinatorDiagnostics -match '127\.0\.0\.1|example\.invalid|192\.0\.2\.1|synthetic-isolation') { throw 'Sensitive endpoint/payload appeared in coordinator diagnostics.' }
    $result | Add-Member -NotePropertyName unhosted_coordinator -NotePropertyValue $coordinatorResult
    & dumpbin.exe /nologo /dependents $binary | Tee-Object -FilePath (Join-Path $outputRoot 'runtime-imports.txt')
    if ($LASTEXITCODE) { throw 'Runtime audit failed.' }
    $result | Add-Member -NotePropertyName native_binary_sha256 -NotePropertyValue (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant()
    $result | Add-Member -NotePropertyName verified_utc -NotePropertyValue ([DateTime]::UtcNow.ToString('O'))
    if (-not $VerificationBase) {
        $baseRecord = Join-Path $outputRoot 'combined-base.json'
        if (Test-Path -LiteralPath $baseRecord) { $VerificationBase = (Get-Content -LiteralPath $baseRecord -Raw | ConvertFrom-Json).base }
    }
    $result | Add-Member -NotePropertyName pinned_base -NotePropertyValue $VerificationBase
    $result | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $outputRoot 'verification.json') -Encoding utf8
    Write-Output ('VERIFIED_PROBE_CALLER=' + (Join-Path $outputRoot 'verification.json'))
} finally {
    foreach ($variable in $variables) { [Environment]::SetEnvironmentVariable($variable,$previous[$variable],'Process') }
    $slotHandle.Dispose()
}
