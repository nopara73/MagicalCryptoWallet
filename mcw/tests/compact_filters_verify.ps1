param(
    [Parameter(Mandatory = $true)][string]$ToolchainBin,
    [Parameter(Mandatory = $true)][string]$CoordinationRoot,
    [string]$EncodingSource,
    [string]$ExpectedEncodingSha256,
    [string]$Linker,
    [string[]]$NativeLibraryPaths = @(),
    [string]$ReferenceDirectory,
    [string]$Python = 'python'
)
$ErrorActionPreference = 'Stop'
$taskRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$taskEvidence = Join-Path $taskRoot '.artifacts/compact-filters-evidence'
New-Item -ItemType Directory -Force -Path $taskEvidence | Out-Null
if (-not $EncodingSource) { $EncodingSource = Join-Path $taskRoot 'mcw/src/bitcoin_encoding.rs' }
$taskEncoding = (Resolve-Path -LiteralPath $EncodingSource).Path
$taskSourceBefore = (Get-FileHash -LiteralPath $taskEncoding -Algorithm SHA256).Hash.ToLowerInvariant()
if ($ExpectedEncodingSha256 -and $taskSourceBefore -ne $ExpectedEncodingSha256.ToLowerInvariant()) {
    throw "Encoding source hash changed: $taskSourceBefore"
}
$taskRustc = Join-Path $ToolchainBin 'rustc.exe'
$taskRustfmt = Join-Path $ToolchainBin 'rustfmt.exe'
$taskClippy = Join-Path $ToolchainBin 'clippy-driver.exe'
$taskVersion = & $taskRustc --version
if ($LASTEXITCODE -ne 0 -or $taskVersion -notmatch '^rustc 1\.99\.0 ') { throw "Expected Rust 1.99.0; got $taskVersion" }
$taskFreeGiB = (Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB
if ($taskFreeGiB -lt 2) { Write-Output ('BUILD_DEFERRED: {0:N2} GiB free' -f $taskFreeGiB); exit 3 }
$taskSlot = $null
foreach ($taskNumber in @(1, 2)) {
    try {
        $taskSlot = [IO.File]::Open((Join-Path $CoordinationRoot "build-slot-$taskNumber.lock"),
            [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        break
    } catch [IO.IOException] {}
}
if ($null -eq $taskSlot) { Write-Output 'BUILD_SLOTS_BUSY'; exit 3 }
try {
    $env:CARGO_BUILD_JOBS = '1'
    Write-Output $taskVersion
    Write-Output "SHA256_SOURCE=$taskEncoding"
    Write-Output "SHA256_SOURCE_HASH=$taskSourceBefore"
    $taskModule = Join-Path $taskRoot 'mcw/src/compact_filters.rs'
    $taskTests = Join-Path $PSScriptRoot 'compact_filters_conformance.rs'
    & $taskRustfmt --edition 2024 --check $taskModule $taskTests
    if ($LASTEXITCODE -ne 0) { throw 'Formatting check failed' }
    # A temporary test crate in ignored evidence storage, never a shipping host.
    $taskHarness = Join-Path $taskEvidence 'harness.rs'
    $taskHarnessText = @(
        'extern crate self as mcw;',
        ('#[path = "{0}"] pub mod bitcoin_encoding;' -f $taskEncoding.Replace('\','/')),
        ('#[path = "{0}"] pub mod compact_filters;' -f $taskModule.Replace('\','/')),
        '#[cfg(test)]',
        ('#[path = "{0}"] mod compact_filters_conformance;' -f $taskTests.Replace('\','/'))
    ) -join "`n"
    [IO.File]::WriteAllText($taskHarness, $taskHarnessText, [Text.UTF8Encoding]::new($false))
    $taskNativeArguments = @('-C', 'target-feature=+crt-static')
    if ($Linker) {
        if (-not (Test-Path -LiteralPath $Linker)) { throw "Missing native linker $Linker" }
        $taskNativeArguments += @('-C', "linker=$Linker")
    }
    foreach ($taskNativePath in $NativeLibraryPaths) {
        if (-not (Test-Path -LiteralPath $taskNativePath)) { throw "Missing native library path $taskNativePath" }
        $taskNativeArguments += @('-L', "native=$taskNativePath")
    }
    foreach ($taskLintMode in @('library', 'tests')) {
        $taskLintArguments = @('--edition=2024', '--emit=metadata', '-D', 'warnings', '-W', 'clippy::all',
            $taskHarness, '-o', (Join-Path $taskEvidence "clippy-$taskLintMode.rmeta"))
        if ($taskLintMode -eq 'library') { $taskLintArguments += '--crate-type=lib' }
        else { $taskLintArguments += '--test' }
        & $taskClippy @taskLintArguments 2>&1 | Tee-Object -FilePath (Join-Path $taskEvidence "clippy-$taskLintMode.txt")
        if ($LASTEXITCODE -ne 0) { throw "Clippy $taskLintMode verification failed" }
    }
    $taskProfiles = @()
    foreach ($taskProfile in @('debug', 'optimized')) {
        $taskBinary = Join-Path $taskEvidence "compact-filters-$taskProfile.exe"
        $taskArguments = @('--edition=2024', '--test', $taskHarness, '-D', 'warnings',
            '-C', 'overflow-checks=yes', '-o', $taskBinary) + $taskNativeArguments
        if ($taskProfile -eq 'optimized') { $taskArguments += @('-C', 'opt-level=3') }
        & $taskRustc @taskArguments
        if ($LASTEXITCODE -ne 0) { throw "$taskProfile compilation failed" }
        & $taskBinary --test-threads=1 compact_filters_conformance 2>&1 |
            Tee-Object -FilePath (Join-Path $taskEvidence "$taskProfile-tests.txt")
        if ($LASTEXITCODE -ne 0) { throw "$taskProfile conformance tests failed" }
        $taskProfiles += [ordered]@{ profile = $taskProfile; state = 'passed' }
    }
    $taskSysroot = & $taskRustc --print sysroot
    $taskTargets = @('x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu',
        'aarch64-unknown-linux-gnu', 'x86_64-apple-darwin', 'aarch64-apple-darwin')
    $taskTargetResults = @()
    foreach ($taskTarget in $taskTargets) {
        if (-not (Test-Path -LiteralPath (Join-Path $taskSysroot "lib/rustlib/$taskTarget/lib"))) {
            $taskTargetResults += [ordered]@{ target = $taskTarget; state = 'unavailable'; reason = 'target std not installed' }
            continue
        }
        & $taskRustc --edition=2024 --crate-type=lib --crate-name compact_filters_audit --emit=metadata `
            --target $taskTarget -D warnings $taskHarness -o (Join-Path $taskEvidence "$taskTarget.rmeta")
        if ($LASTEXITCODE -ne 0) { throw "$taskTarget metadata compilation failed" }
        $taskTargetResults += [ordered]@{ target = $taskTarget; state = 'metadata_pass'; reason = 'no linker/runtime test' }
    }
    $taskReference = 'not_regenerated'
    if ($ReferenceDirectory) {
        $taskGenerated = Join-Path $taskEvidence 'regenerated-vectors.inc'
        & $Python (Join-Path $PSScriptRoot 'compact_filters_reference.py') `
            --bip158 (Join-Path $ReferenceDirectory 'testnet-19.json') `
            --siphash (Join-Path $ReferenceDirectory 'siphash-vectors.h') --output $taskGenerated 2>&1 |
            Tee-Object -FilePath (Join-Path $taskEvidence 'independent-reference.txt')
        if ($LASTEXITCODE -ne 0) { throw 'Independent reference generation failed' }
        $taskExpected = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'compact_filters_vectors.inc')).Replace("`r`n","`n")
        $taskActual = [IO.File]::ReadAllText($taskGenerated).Replace("`r`n","`n")
        if ($taskExpected -cne $taskActual) { throw 'Committed fixtures differ from independent reference output' }
        $taskReference = 'verified_identical'
    }
    $taskSourceAfter = (Get-FileHash -LiteralPath $taskEncoding -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($taskSourceBefore -ne $taskSourceAfter) { throw 'Encoding source changed during verification; rerun required' }
    $taskSummary = [ordered]@{
        rust = $taskVersion
        encoding_source = $taskEncoding
        encoding_source_sha256 = $taskSourceAfter
        compact_filters_sha256 = (Get-FileHash -LiteralPath $taskModule -Algorithm SHA256).Hash.ToLowerInvariant()
        clippy = 'library_and_tests_passed'
        profiles = $taskProfiles
        targets = $taskTargetResults
        independent_reference = $taskReference
    }
    $taskSummary | ConvertTo-Json -Depth 6 |
        Set-Content -LiteralPath (Join-Path $taskEvidence 'verification.json') -Encoding utf8NoBOM
    $taskSummary | ConvertTo-Json -Depth 6 -Compress | Write-Output
} finally { $taskSlot.Dispose() }
