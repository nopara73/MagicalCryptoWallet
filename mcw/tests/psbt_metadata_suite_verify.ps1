param([string]$NativeApplication, [string]$CoreSourceRoot, [ValidateSet('MagicalCryptoWallet.Tests','MagicalCryptoWallet.IntegrationTests')][string]$TestProject = 'MagicalCryptoWallet.Tests', [string[]]$FilterClass = @('*TransactionFactoryTests','*WalletOperationAuthorizationTests','*SoftwareWalletTests'), [string]$FilterNamespace, [switch]$AllTests, [switch]$BuildOnly, [ValidateRange(0,300)][int]$WaitForSlotSeconds = 0)
$ErrorActionPreference = 'Stop'
$metadataRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$metadataSource = if ($CoreSourceRoot) { [IO.Path]::GetFullPath($CoreSourceRoot) } else { $metadataRoot }
$metadataFactory = Join-Path $metadataSource 'MagicalCryptoWallet/Blockchain/Transactions/TransactionFactory.cs'
if (-not [IO.File]::ReadAllText($metadataFactory).Contains('psbt = MagicalCryptoWallet.Mcw.Psbt.McwPsbtMetadata.Enrich(')) { throw 'Suite verification requires the atomically activated native factory.' }
$metadataProject = Join-Path $metadataSource "$TestProject/$TestProject.csproj"
$metadataInitializer = Join-Path $metadataSource "$TestProject/ModuleInitializer.cs"
if (-not [IO.File]::ReadAllText($metadataProject).Contains('psbt_metadata_managed_fixture.cs') -or -not [IO.File]::ReadAllText($metadataInitializer).Contains('McwManagedTestHost.Initialize();')) { throw 'The real-host suite fixture must be integrated with the caller.' }
if (-not $BuildOnly -and (-not $NativeApplication -or -not (Test-Path -LiteralPath $NativeApplication -PathType Leaf))) { throw 'A fresh actual mcw application is required.' }
$metadataCommon = & git -C $metadataRoot rev-parse --path-format=absolute --git-common-dir
if ($LASTEXITCODE -ne 0) { throw 'Cannot locate shared build coordination root.' }
$metadataShared = Split-Path -Parent ($metadataCommon | Select-Object -First 1)
$metadataRun = (Get-Date -Format 'yyyyMMdd-HHmmss-fff') + '-' + [guid]::NewGuid().ToString('N').Substring(0,8)
$metadataOutput = Join-Path $metadataRoot ".artifacts/psbt-metadata-suite/$metadataRun"
New-Item -ItemType Directory -Path $metadataOutput -Force | Out-Null
$metadataSlot = $null
$metadataDeadline = [DateTime]::UtcNow.AddSeconds($WaitForSlotSeconds)
do {
    foreach ($metadataIndex in 1..2) {
        try { $metadataSlot = [IO.File]::Open((Join-Path $metadataShared ".artifacts/mcw-coordination/build-slot-$metadataIndex.lock"), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None); break } catch [IO.IOException] { }
    }
    if ($metadataSlot -or [DateTime]::UtcNow -ge $metadataDeadline) { break }
    Start-Sleep -Seconds 2
} while ($true)
if (-not $metadataSlot) { throw 'Both build slots occupied; preserve the suite snapshot and retry later.' }
try {
    Write-Output "BUILD_SLOT=$metadataIndex VERIFIER_PID=$PID ASSIGNMENT=psbt_metadata_real_managed_suite"
    if ([OperatingSystem]::IsWindows()) {
        $metadataFreeMemory = (Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory * 1KB
    } elseif ([OperatingSystem]::IsLinux()) {
        $metadataMemory = [IO.File]::ReadAllText('/proc/meminfo')
        $metadataAvailable = [regex]::Match($metadataMemory, '(?m)^MemAvailable:\s*(\d+)\s+kB')
        if (-not $metadataAvailable.Success) { throw 'Cannot verify available physical memory.' }
        $metadataFreeMemory = [long]$metadataAvailable.Groups[1].Value * 1KB
    } elseif ([OperatingSystem]::IsMacOS()) {
        $metadataMemory = (& vm_stat) -join "`n"
        if ($LASTEXITCODE -ne 0) { throw 'Cannot read available physical memory.' }
        $metadataPage = [regex]::Match($metadataMemory, 'page size of (\d+) bytes')
        if (-not $metadataPage.Success) { throw 'Cannot verify physical memory page size.' }
        $metadataPages = [regex]::Matches($metadataMemory, '(?m)^Pages (?:free|inactive|speculative):\s*(\d+)\.')
        if ($metadataPages.Count -ne 3) { throw 'Cannot verify free/reclaimable physical memory.' }
        $metadataFreeMemory = ($metadataPages | ForEach-Object { [long]$_.Groups[1].Value } | Measure-Object -Sum).Sum * [long]$metadataPage.Groups[1].Value
    } else { throw 'Unsupported verification host.' }
    if ($metadataFreeMemory -lt 2GB) { throw 'Less than 2 GiB available physical RAM; defer compilation.' }
    $metadataBin = Join-Path $metadataOutput 'bin'
    & dotnet build $metadataProject -c Release -m:1 /p:UseSharedCompilation=false /p:BuildMcwHost=false -o $metadataBin 2>&1 | Tee-Object -FilePath (Join-Path $metadataOutput 'managed-suite-build.log')
    if ($LASTEXITCODE -ne 0) { throw 'Actual managed suite build failed.' }
    $metadataEvidence = [ordered]@{core_source_root=$metadataSource;source_selection=$(if ($CoreSourceRoot) { 'isolated_candidate' } else { 'checkout' });test_project=$TestProject;factory_sha256=(Get-FileHash $metadataFactory).Hash.ToLowerInvariant();fixture_sha256=(Get-FileHash (Join-Path $metadataSource 'mcw/tests/psbt_metadata_managed_fixture.cs')).Hash.ToLowerInvariant();project_sha256=(Get-FileHash $metadataProject).Hash.ToLowerInvariant();initializer_sha256=(Get-FileHash $metadataInitializer).Hash.ToLowerInvariant();suite_build='passed';native_suite_verified=$false;product_release=$false;results=@()}
    if (-not $BuildOnly) {
        $metadataExtension = if ([OperatingSystem]::IsWindows()) { '.exe' } else { '' }
        $metadataChild = Join-Path $metadataBin "$TestProject$metadataExtension"
        $metadataAlias = Join-Path $metadataBin "MagicalCryptoWallet.Fluent.Desktop$metadataExtension"
        if (Test-Path -LiteralPath $metadataAlias) { throw 'Unexpected child alias in unique test output.' }
        Copy-Item -LiteralPath $metadataChild -Destination $metadataAlias
        if (-not [OperatingSystem]::IsWindows()) { [IO.File]::SetUnixFileMode($metadataAlias, [IO.File]::GetUnixFileMode($metadataChild)) }
        $metadataNative = Join-Path $metadataBin "mcw$metadataExtension"
        Copy-Item -LiteralPath $NativeApplication -Destination $metadataNative
        if (-not [OperatingSystem]::IsWindows()) { [IO.File]::SetUnixFileMode($metadataNative, [IO.File]::GetUnixFileMode([IO.Path]::GetFullPath($NativeApplication))) }
        $env:DOTNET_CLI_UI_LANGUAGE = 'en-US'
        $metadataFilters = @(if ($FilterNamespace) { ,@('--filter-namespace', $FilterNamespace) } elseif ($AllTests) { ,@() } else { foreach ($metadataClass in $FilterClass) { ,@('--filter-class', $metadataClass) } })
        foreach ($metadataFilter in $metadataFilters) {
            $metadataLabel = if ($metadataFilter.Count) { $metadataFilter[1] } else { 'all_tests' }
            $metadataLog = Join-Path $metadataOutput (($metadataLabel -replace '[^a-zA-Z0-9]','_') + '.log')
            & $metadataNative gui @metadataFilter --progress off --no-ansi --output Normal 2>&1 | Tee-Object -FilePath $metadataLog
            if ($LASTEXITCODE -ne 0) { throw "Managed suite failed through the actual host: $metadataLabel" }
            $metadataLogText = [IO.File]::ReadAllText($metadataLog)
            if (-not $metadataLogText.Contains('MCW_MANAGED_TEST_HOST_CONNECTED services=production bridge=stdio') -or -not $metadataLogText.Contains('MCW_MANAGED_TEST_HOST_CLOSED')) { throw 'Real-host connection/cleanup markers missing.' }
            $metadataCount = [regex]::Match($metadataLogText, '(?im)^\s*succeeded:\s*(\d+)')
            if (-not $metadataCount.Success -or [int]$metadataCount.Groups[1].Value -le 0) { throw 'No executed managed tests verified.' }
            $metadataEvidence.results += [ordered]@{filter=$metadataLabel;succeeded=[int]$metadataCount.Groups[1].Value;exit_code=0;log=$metadataLog}
        }
        if (-not $metadataEvidence.results.Count) { throw 'No managed-suite invocation verified.' }
        $metadataEvidence.native_suite_verified=$true
        $metadataEvidence['native_sha256']=(Get-FileHash $metadataNative).Hash.ToLowerInvariant()
    }
    $metadataEvidence | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $metadataOutput 'evidence.json') -Encoding utf8
    Write-Output "MANAGED_SUITE_EVIDENCE=$metadataOutput"
} finally { $metadataSlot.Dispose() }
