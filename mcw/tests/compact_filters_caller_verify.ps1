param(
    [Parameter(Mandatory = $true)][string]$ToolchainBin,
    [Parameter(Mandatory = $true)][string]$CoordinationRoot,
    [Parameter(Mandatory = $true)][string]$Linker,
    [Parameter(Mandatory = $true)][string[]]$NativeLibraryPaths,
    [string]$Python = 'python',
    [ValidateRange(0,120)][int]$WaitForBuildSlotSeconds = 0,
    [string]$ExistingSnapshot = ''
)
$ErrorActionPreference = 'Stop'
$taskRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$taskEvidence = Join-Path $taskRoot '.artifacts/compact-filters-caller-evidence'
New-Item -ItemType Directory -Force -Path $taskEvidence | Out-Null
if ($ExistingSnapshot) {
    $taskSnapshot = (Resolve-Path -LiteralPath $ExistingSnapshot).Path
    if (-not $taskSnapshot.StartsWith($taskEvidence + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
        -not (Split-Path -Leaf $taskSnapshot).StartsWith('snapshot-')) { throw 'Existing snapshot is outside this task evidence directory' }
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'compact_filters_caller_probe.inc') -Destination (Join-Path $taskSnapshot 'caller-probe/Program.cs')
} else {
    $taskSnapshot = Join-Path $taskEvidence ('snapshot-' + [Guid]::NewGuid().ToString('N'))
    & $Python (Join-Path $PSScriptRoot 'compact_filters_caller_prepare.py') --root $taskRoot --snapshot $taskSnapshot
    if ($LASTEXITCODE -ne 0) { throw 'Real Core/Client source snapshot preparation failed' }
}
$taskManifest = Get-Content -LiteralPath (Join-Path $taskSnapshot 'caller-source-manifest.json') -Raw | ConvertFrom-Json
if (((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB) -lt 2) { Write-Output 'BUILD_DEFERRED'; exit 3 }
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
    if (((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB) -lt 2) { Write-Output 'BUILD_DEFERRED'; exit 3 }
    $taskReport = Join-Path $taskEvidence 'caller-results.json'
    if (Test-Path -LiteralPath $taskReport) { Remove-Item -LiteralPath $taskReport }
    $taskSummary = Join-Path $taskEvidence 'caller-verification.json'
    if (Test-Path -LiteralPath $taskSummary) { Remove-Item -LiteralPath $taskSummary }
    $env:CARGO_BUILD_JOBS = '1'
    $env:RUSTC = Join-Path $ToolchainBin 'rustc.exe'
    $env:MCW_WINDOWS_RUNTIME = '0'
    $taskNativeFlags = @('-C','target-feature=+crt-static','-C',"linker=$Linker")
    foreach ($taskNativePath in $NativeLibraryPaths) { $taskNativeFlags += @('-L',"native=$taskNativePath") }
    $env:CARGO_ENCODED_RUSTFLAGS = $taskNativeFlags -join [char]31
    $env:CARGO_TARGET_DIR = Join-Path $taskRoot '.artifacts/compact-filters-caller-host-target'
    $taskCargo = Join-Path $ToolchainBin 'cargo.exe'
    & $taskCargo build --manifest-path (Join-Path $taskSnapshot 'mcw/Cargo.toml') --locked --offline -j 1 --bin mcw 2>&1 |
        Tee-Object -FilePath (Join-Path $taskEvidence 'caller-rust-host-build.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Pinned real Rust host with prepared handler failed to build' }

    $taskCore = Join-Path $taskSnapshot 'MagicalCryptoWallet/MagicalCryptoWallet.csproj'
    $taskProbe = Join-Path $taskSnapshot 'caller-probe/CallerProbe.csproj'
    $taskConfig = Join-Path $taskSnapshot 'NuGet.Config'
    # These are existing retained locked packages from the local NuGet cache.
    # Clear sources and suppress feed audit only for offline verification; no
    # production package manifest/lock or package audit policy is changed.
    $taskFlags = @('-p:NuGetAudit=false','-p:BuildMcwHost=false','-p:UseSharedCompilation=false','-m:1')
    dotnet restore $taskCore --locked-mode --configfile $taskConfig @taskFlags --verbosity minimal 2>&1 |
        Tee-Object -FilePath (Join-Path $taskEvidence 'actual-core-restore.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Offline locked actual Core restore failed' }
    dotnet build $taskCore --no-restore -c Debug @taskFlags --verbosity minimal 2>&1 |
        Tee-Object -FilePath (Join-Path $taskEvidence 'actual-core-build.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Actual complete Core with patched WalletFilterProcessor failed to compile' }
    dotnet restore $taskProbe --configfile $taskConfig @taskFlags --verbosity minimal 2>&1 |
        Tee-Object -FilePath (Join-Path $taskEvidence 'caller-probe-restore.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Offline actual Client / retained-test probe restore failed' }
    $taskBin = Join-Path $taskSnapshot 'caller-bin'
    dotnet publish $taskProbe --no-restore -c Debug -o $taskBin @taskFlags --verbosity minimal 2>&1 |
        Tee-Object -FilePath (Join-Path $taskEvidence 'actual-client-and-caller-probe-build.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Actual Client and actual retained-test/caller probe compilation failed' }
    # Friend-assembly name grants existing test access. The copied apphost still
    # loads that same assembly; this synthetic alias is never shipped.
    Copy-Item -LiteralPath (Join-Path $taskBin 'MagicalCryptoWallet.Tests.exe') -Destination (Join-Path $taskBin 'magicalcryptowalletd.exe')
    Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'debug/mcw.exe') -Destination $taskBin
    # A short ignored state path keeps unchanged retained SQLite tests below
    # the native Windows path limit; the source snapshot remains isolated.
    $taskSharedWorkspace = (Resolve-Path (Join-Path $CoordinationRoot '../..')).Path
    $taskStateRoot = [IO.Path]::GetFullPath((Join-Path $taskSharedWorkspace ('.artifacts/cfs-' + [Guid]::NewGuid().ToString('N').Substring(0,8))))
    if (-not $taskStateRoot.StartsWith($taskSharedWorkspace + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
        (Test-Path -LiteralPath $taskStateRoot)) { throw 'Synthetic state path is not a fresh workspace-owned directory' }
    $taskArguments = @('daemon',('"' + $taskReport + '"'),('"' + $taskStateRoot + '"'))
    $taskHost = Start-Process -FilePath (Join-Path $taskBin 'mcw.exe') -ArgumentList $taskArguments -WorkingDirectory $taskBin -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskEvidence 'caller-host-stdout.txt') -RedirectStandardError (Join-Path $taskEvidence 'caller-host-stderr.txt')
    try {
        $taskElapsed = [Diagnostics.Stopwatch]::StartNew()
        while (-not $taskHost.WaitForExit(1000)) {
            if ($taskElapsed.Elapsed.TotalSeconds -gt 240) { $taskHost.Kill($true); throw 'Synthetic actual-caller run exceeded its time limit' }
        }
        if ($taskHost.ExitCode -ne 0) {
            Get-Content -LiteralPath (Join-Path $taskEvidence 'caller-host-stderr.txt') -Tail 60
            throw "Actual WalletFilterProcessor / Rust-host run failed: $($taskHost.ExitCode)"
        }
        if (-not (Test-Path -LiteralPath $taskReport)) { throw 'Actual caller exited without results' }
    } finally { $taskHost.Dispose() }
    $taskPatchedExisting = @('mcw/src/app.rs','mcw/src/bridge.rs','MagicalCryptoWallet/Wallets/WalletFilterProcessor.cs')
    foreach ($taskFile in $taskManifest.copied_source_files) {
        if ((Get-FileHash -LiteralPath (Join-Path $taskRoot $taskFile.path)).Hash.ToLowerInvariant() -ne $taskFile.sha256) {
            throw "Reserved existing source changed during caller proof: $($taskFile.path)"
        }
        if ($taskFile.path -notin $taskPatchedExisting -and
            (Get-FileHash -LiteralPath (Join-Path $taskSnapshot $taskFile.path)).Hash.ToLowerInvariant() -ne $taskFile.sha256) {
            throw "Unexpected copied source or retained lock mutation: $($taskFile.path)"
        }
    }
    if ((Get-FileHash -LiteralPath (Join-Path $taskSnapshot 'MagicalCryptoWallet/Wallets/WalletFilterProcessor.cs')).Hash.ToLowerInvariant() -ne $taskManifest.patched_caller_sha256 -or
        (Get-FileHash -LiteralPath (Join-Path $taskSnapshot 'MagicalCryptoWallet/Mcw/CompactFilters/McwCompactFilterMatcher.cs')).Hash.ToLowerInvariant() -ne $taskManifest.patched_adapter_sha256) {
        throw 'Patched caller or adapter bytes changed after snapshot preparation'
    }
    $taskCaller = Get-Content -LiteralPath $taskReport -Raw | ConvertFrom-Json
    $taskResult = [ordered]@{
        source_pin = $taskManifest.source_pin
        source_manifest = Join-Path $taskSnapshot 'caller-source-manifest.json'
        actual_core_build = 'passed_with_repository_warnings_as_errors_and_locked_retained_dependencies'
        actual_client_build = 'passed'
        patched_caller_sha256 = $taskManifest.patched_caller_sha256
        patched_adapter_sha256 = $taskManifest.patched_adapter_sha256
        patch_sha256_canonical_lf = $taskManifest.patch_sha256_canonical_lf
        caller_assembly_sha256 = $taskCaller.caller_assembly_sha256
        caller_cases = $taskCaller.caller_cases
        retained_tests = $taskCaller.retained_tests
        reserved_existing_sources = 'unchanged'
        snapshot_unpatched_sources_and_retained_locks = 'unchanged'
        compiled_probe_source_sha256 = (Get-FileHash -LiteralPath (Join-Path $taskSnapshot 'caller-probe/Program.cs')).Hash.ToLowerInvariant()
        current_host_binding_restored = $taskCaller.current_host_binding_restored
        scope = 'actual_selected_caller_proved_on_synthetic_state; production_hook_remains_review_patch; NBitcoin_retained'
    }
    $taskResult | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $taskSummary -Encoding utf8NoBOM
    $taskResult | ConvertTo-Json -Depth 10 -Compress | Write-Output
} finally { $taskSlot.Dispose() }
