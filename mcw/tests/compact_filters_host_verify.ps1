param(
    [Parameter(Mandatory = $true)][string]$ToolchainBin,
    [Parameter(Mandatory = $true)][string]$CoordinationRoot,
    [Parameter(Mandatory = $true)][string]$ReferenceDirectory,
    [Parameter(Mandatory = $true)][string]$Linker,
    [Parameter(Mandatory = $true)][string[]]$NativeLibraryPaths,
    [string]$Python = 'python',
    [ValidateRange(0,120)][int]$WaitForBuildSlotSeconds = 0
)
$ErrorActionPreference = 'Stop'
$taskRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$taskEvidence = Join-Path $taskRoot '.artifacts/compact-filters-host-evidence'
New-Item -ItemType Directory -Force -Path $taskEvidence | Out-Null
$taskPin = (git -C $taskRoot rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0) { throw 'Cannot identify published source pin' }
$taskOwnedPaths = @('mcw/src/bridge.rs','mcw/src/app.rs','MagicalCryptoWallet/Wallets/WalletFilterProcessor.cs')
$taskSourceHashes = @{}
foreach ($taskPath in $taskOwnedPaths) { $taskSourceHashes[$taskPath] = (Get-FileHash -LiteralPath (Join-Path $taskRoot $taskPath)).Hash }
& $Python (Join-Path $PSScriptRoot 'compact_filters_host_prepare.py') --root $taskRoot --reference-directory $ReferenceDirectory --output-directory $taskEvidence
if ($LASTEXITCODE -ne 0) { throw 'Review patch preparation failed' }
$taskFreeGiB = (Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB
if ($taskFreeGiB -lt 2) { Write-Output 'BUILD_DEFERRED'; exit 3 }
$taskSlot = $null
$taskWaitUntil = [DateTime]::UtcNow.AddSeconds($WaitForBuildSlotSeconds)
do {
    foreach ($taskNumber in @(1,2)) {
        try {
            $taskSlot = [IO.File]::Open((Join-Path $CoordinationRoot "build-slot-$taskNumber.lock"),
                [IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
            break
        } catch [IO.IOException] {}
    }
    if ($null -ne $taskSlot -or [DateTime]::UtcNow -ge $taskWaitUntil) { break }
    Start-Sleep -Milliseconds 1000
} while ($true)
if ($null -eq $taskSlot) { Write-Output 'BUILD_SLOTS_BUSY'; exit 3 }
try {
    if (((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB) -lt 2) {
        Write-Output 'BUILD_DEFERRED'; exit 3
    }
    foreach ($taskOldReport in @('adapter-unit-results.json','host-roundtrip-results.json','host-verification.json')) {
        $taskOldPath = Join-Path $taskEvidence $taskOldReport
        if (Test-Path -LiteralPath $taskOldPath) { Remove-Item -LiteralPath $taskOldPath }
    }
    $env:CARGO_BUILD_JOBS = '1'
    $env:RUSTC = Join-Path $ToolchainBin 'rustc.exe'
    $env:MCW_WINDOWS_RUNTIME = '0'
    $taskNativeFlags = @('-C','target-feature=+crt-static','-C',"linker=$Linker")
    foreach ($taskNativePath in $NativeLibraryPaths) { $taskNativeFlags += @('-L',"native=$taskNativePath") }
    $env:CARGO_ENCODED_RUSTFLAGS = $taskNativeFlags -join [char]31
    $taskCargo = Join-Path $ToolchainBin 'cargo.exe'
    $env:CARGO_TARGET_DIR = Join-Path $taskRoot '.artifacts/compact-filters-host-target'
    & $taskCargo test --manifest-path (Join-Path $taskRoot 'mcw/Cargo.toml') --locked --offline -j 1 --test compact_filters_conformance -- --test-threads=1 2>&1 |
        Tee-Object -FilePath (Join-Path $taskEvidence 'cargo-debug-tests.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Actual published-host Cargo conformance failed' }
    $taskGraphText = & $taskCargo metadata --manifest-path (Join-Path $taskRoot 'mcw/Cargo.toml') --locked --offline --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Cargo dependency audit failed' }
    $taskGraph = ($taskGraphText -join "`n") | ConvertFrom-Json
    if ($taskGraph.packages.Count -ne 1 -or $taskGraph.packages[0].dependencies.Count -ne 0) { throw 'External Cargo dependencies present' }

    # Apply the proposed owner patch only to an ignored snapshot, never tracked
    # host/adapter/caller files, the shared checkout, or the active QR checkout.
    $taskScratch = Join-Path $taskEvidence ('scratch-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force -Path $taskScratch | Out-Null
    Copy-Item -LiteralPath (Join-Path $taskRoot 'mcw') -Destination (Join-Path $taskScratch 'mcw') -Recurse
    $taskCallerDirectory = Join-Path $taskScratch 'MagicalCryptoWallet/Wallets'
    New-Item -ItemType Directory -Force -Path $taskCallerDirectory | Out-Null
    Copy-Item -LiteralPath (Join-Path $taskRoot 'MagicalCryptoWallet/Wallets/WalletFilterProcessor.cs') -Destination $taskCallerDirectory
    $taskAdapterDirectory = Join-Path $taskScratch 'MagicalCryptoWallet/Mcw/CompactFilters'
    New-Item -ItemType Directory -Force -Path $taskAdapterDirectory | Out-Null
    Push-Location $taskScratch
    try {
        git apply --no-index --check --ignore-space-change (Join-Path $PSScriptRoot 'compact_filters_host_wiring.patch')
        if ($LASTEXITCODE -ne 0) { throw 'Patch does not apply to the pinned source snapshot' }
        git apply --no-index --ignore-space-change (Join-Path $PSScriptRoot 'compact_filters_host_wiring.patch')
        if ($LASTEXITCODE -ne 0) { throw 'Scratch patch application failed' }
    } finally { Pop-Location }
    $env:CARGO_TARGET_DIR = Join-Path $taskRoot '.artifacts/compact-filters-patched-host-target'
    & $taskCargo test --manifest-path (Join-Path $taskScratch 'mcw/Cargo.toml') --locked --offline -j 1 --test compact_filters_conformance -- --test-threads=1 2>&1 |
        Tee-Object -FilePath (Join-Path $taskEvidence 'patched-cargo-tests.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Patched-host Cargo conformance failed' }
    & $taskCargo build --manifest-path (Join-Path $taskScratch 'mcw/Cargo.toml') --locked --offline -j 1 --bin mcw 2>&1 |
        Tee-Object -FilePath (Join-Path $taskEvidence 'patched-host-build.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Patched application host build failed' }

    $taskProbe = Join-Path $taskScratch 'probe'
    New-Item -ItemType Directory -Force -Path $taskProbe | Out-Null
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'compact_filters_host_probe.inc') -Destination (Join-Path $taskProbe 'Program.cs')
    Copy-Item -LiteralPath (Join-Path $taskRoot 'MagicalCryptoWallet.Client/Application/ManagedApplicationHost.cs') -Destination $taskProbe
    Copy-Item -LiteralPath (Join-Path $taskRoot 'MagicalCryptoWallet/Mcw/IMcwApplicationServices.cs') -Destination $taskProbe
    Copy-Item -LiteralPath (Join-Path $taskAdapterDirectory 'McwCompactFilterMatcher.cs') -Destination $taskProbe
    [IO.File]::WriteAllText((Join-Path $taskProbe 'SyntheticTermination.cs'), 'namespace MagicalCryptoWallet.Services.Terminate { public sealed class TerminateService { public void SignalForceTerminate() {} } }')
    [IO.File]::WriteAllText((Join-Path $taskProbe 'NuGet.Config'), '<configuration><packageSources><clear /></packageSources></configuration>')
    $taskProject = '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><AssemblyName>magicalcryptowallet</AssemblyName><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup></Project>'
    [IO.File]::WriteAllText((Join-Path $taskProbe 'Probe.csproj'), $taskProject)
    $taskMsbuildFlags = @('-p:ImportDirectoryBuildProps=false','-p:ImportDirectoryBuildTargets=false','-p:ManagePackageVersionsCentrally=false')
    dotnet restore (Join-Path $taskProbe 'Probe.csproj') --configfile (Join-Path $taskProbe 'NuGet.Config') @taskMsbuildFlags --verbosity quiet 2>&1 |
        Tee-Object -FilePath (Join-Path $taskEvidence 'managed-probe-restore.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Offline synthetic probe restore failed' }
    $taskBin = Join-Path $taskScratch 'bin'
    dotnet publish (Join-Path $taskProbe 'Probe.csproj') --no-restore -o $taskBin @taskMsbuildFlags --verbosity quiet 2>&1 |
        Tee-Object -FilePath (Join-Path $taskEvidence 'managed-probe-build.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Synthetic typed adapter/real managed-host source compilation failed' }
    Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'debug/mcw.exe') -Destination $taskBin
    $taskCases = Join-Path $taskEvidence 'host-cases.json'
    & (Join-Path $taskBin 'magicalcryptowallet.exe') unit $taskCases (Join-Path $taskEvidence 'adapter-unit-results.json')
    if ($LASTEXITCODE -ne 0) { throw 'Typed adapter unit contract failed' }
    # mcw is a Windows GUI-subsystem executable. A plain
    # invocation can return before its synthetic managed child writes results.
    $taskRoundtripReport = Join-Path $taskEvidence 'host-roundtrip-results.json'
    $taskHostArguments = @('gui','roundtrip',('"' + $taskCases + '"'),('"' + $taskRoundtripReport + '"'))
    $taskHost = Start-Process -FilePath (Join-Path $taskBin 'mcw.exe') -ArgumentList $taskHostArguments -WorkingDirectory $taskBin -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskEvidence 'host-roundtrip-stdout.txt') -RedirectStandardError (Join-Path $taskEvidence 'host-roundtrip-stderr.txt')
    try {
        $taskElapsed = [Diagnostics.Stopwatch]::StartNew()
        while (-not $taskHost.WaitForExit(1000)) {
            if ($taskElapsed.Elapsed.TotalSeconds -gt 180) {
                $taskHost.Kill($true)
                throw 'Synthetic host roundtrip exceeded its time limit'
            }
        }
        if ($taskHost.ExitCode -ne 0) { throw "Real mcw host / managed adapter roundtrip failed: $($taskHost.ExitCode)" }
        if (-not (Test-Path -LiteralPath $taskRoundtripReport)) { throw 'Successful host exit did not produce roundtrip evidence' }
    } finally { $taskHost.Dispose() }
    foreach ($taskPath in $taskOwnedPaths) {
        if ($taskSourceHashes[$taskPath] -ne (Get-FileHash -LiteralPath (Join-Path $taskRoot $taskPath)).Hash) { throw "Reserved tracked file changed: $taskPath" }
    }
    $taskResults = [ordered]@{
        source_pin = $taskPin
        cargo_tests_per_run = 19
        actual_host_cargo = 'passed'
        patched_host_cargo = 'passed'
        cargo_packages = $taskGraph.packages.Count
        cargo_external_dependencies = 0
        adapter_unit = Get-Content -LiteralPath (Join-Path $taskEvidence 'adapter-unit-results.json') -Raw | ConvertFrom-Json
        host_roundtrip = Get-Content -LiteralPath (Join-Path $taskEvidence 'host-roundtrip-results.json') -Raw | ConvertFrom-Json
        reserved_tracked_files = 'unchanged'
        scratch_directory = $taskScratch
        patch_sha256 = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'compact_filters_host_wiring.patch')).Hash.ToLowerInvariant()
        scope = 'prepared_matching_caller_and_handler_patch_only; production_package_not_migrated'
    }
    $taskResults | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $taskEvidence 'host-verification.json') -Encoding utf8NoBOM
    $taskResults | ConvertTo-Json -Depth 8 -Compress | Write-Output
} finally { $taskSlot.Dispose() }
