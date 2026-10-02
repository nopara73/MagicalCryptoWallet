param(
    [Parameter(Mandatory = $true)][string]$ToolchainBin,
    [Parameter(Mandatory = $true)][string]$CoordinationRoot,
    [string]$ReferenceDirectory,
    [string[]]$NativeLibraryPaths = @()
)
$ErrorActionPreference = 'Stop'
$taskRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$taskEvidence = Join-Path $taskRoot '.artifacts/bitcoin-encoding-evidence'
New-Item -ItemType Directory -Force -Path $taskEvidence | Out-Null
$taskRustc = Join-Path $ToolchainBin 'rustc.exe'
$taskRustfmt = Join-Path $ToolchainBin 'rustfmt.exe'
$taskClippy = Join-Path $ToolchainBin 'clippy-driver.exe'
$taskVersion = & $taskRustc --version
if ($LASTEXITCODE -ne 0 -or $taskVersion -notmatch '^rustc 1\.99\.0 ') { throw "Expected Rust 1.99.0; got $taskVersion" }
Write-Output $taskVersion
$taskFreeGiB = (Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB
if ($taskFreeGiB -lt 2) {
    Write-Output ('BUILD_DEFERRED: {0:N2} GiB free' -f $taskFreeGiB)
    exit 3
}
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
    $taskNativeArguments = @('-C', 'target-feature=+crt-static')
    foreach ($taskNativePath in $NativeLibraryPaths) {
        if (-not (Test-Path -LiteralPath $taskNativePath)) { throw "Missing native build library path $taskNativePath" }
        $taskNativeArguments += @('-L', "native=$taskNativePath")
    }
    & $taskRustfmt --edition 2024 --check (Join-Path $taskRoot 'mcw/src/bitcoin_encoding.rs') (Join-Path $PSScriptRoot 'bitcoin_encoding_conformance.rs')
    if ($LASTEXITCODE -ne 0) { throw 'Formatting check failed' }
    & $taskClippy --edition=2024 --crate-type=lib --crate-name bitcoin_encoding --emit=metadata -D warnings (Join-Path $taskRoot 'mcw/src/bitcoin_encoding.rs') -o (Join-Path $taskEvidence 'clippy-lib.rmeta')
    if ($LASTEXITCODE -ne 0) { throw 'Domain Clippy validation failed' }
    & $taskClippy --edition=2024 --test --emit=metadata -D warnings (Join-Path $PSScriptRoot 'bitcoin_encoding_conformance.rs') -o (Join-Path $taskEvidence 'clippy-tests.rmeta')
    if ($LASTEXITCODE -ne 0) { throw 'Conformance Clippy validation failed' }
    foreach ($taskProfile in @('debug', 'optimized')) {
        $taskBinary = Join-Path $taskEvidence "bitcoin-encoding-$taskProfile.exe"
        $taskArguments = @('--edition=2024', '--test', (Join-Path $PSScriptRoot 'bitcoin_encoding_conformance.rs'),
            '-D', 'warnings', '-C', 'overflow-checks=yes', '-o', $taskBinary)
        $taskArguments += $taskNativeArguments
        if ($taskProfile -eq 'optimized') { $taskArguments += @('-C', 'opt-level=3') }
        & $taskRustc @taskArguments
        if ($LASTEXITCODE -ne 0) { throw "$taskProfile compilation failed" }
        & $taskBinary --test-threads=1 2>&1 | Tee-Object -FilePath (Join-Path $taskEvidence "$taskProfile-tests.txt")
        if ($LASTEXITCODE -ne 0) { throw "$taskProfile conformance tests failed" }
    }
    # Metadata-only compilation proves the exact portable source type-checks on
    # each target with an installed std. It does not prove linking or OS execution.
    $taskSysroot = & $taskRustc --print sysroot
    $taskTargets = @('x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu',
        'x86_64-apple-darwin', 'aarch64-apple-darwin')
    $taskTargetResults = @()
    foreach ($taskTarget in $taskTargets) {
        if (-not (Test-Path -LiteralPath (Join-Path $taskSysroot "lib/rustlib/$taskTarget/lib"))) {
            $taskTargetResults += [ordered]@{ target = $taskTarget; state = 'unavailable'; reason = 'target std not installed' }
            continue
        }
        $taskMetadataArguments = @('--edition=2024', '--crate-type=lib', '--crate-name', 'bitcoin_encoding', '--emit=metadata', '--target', $taskTarget, '-D', 'warnings', (Join-Path $taskRoot 'mcw/src/bitcoin_encoding.rs'), '-o', (Join-Path $taskEvidence "$taskTarget.rmeta"))
        & $taskRustc @taskMetadataArguments
        if ($LASTEXITCODE -ne 0) { throw "$taskTarget metadata compilation failed" }
        $taskTargetResults += [ordered]@{ target = $taskTarget; state = 'metadata_pass'; reason = 'no linker/runtime test' }
    }
    $taskTargetResults | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskEvidence 'target-checks.json') -Encoding utf8NoBOM
    $taskTargetResults | ConvertTo-Json -Compress | Write-Output
    if ($ReferenceDirectory) {
        $taskPythonArguments = @((Join-Path $PSScriptRoot 'bitcoin_encoding_reference.py'), '--rustc', $taskRustc, '--work-directory', $taskEvidence, '--reference-directory', $ReferenceDirectory)
        foreach ($taskNativePath in $NativeLibraryPaths) { $taskPythonArguments += @('--native-lib', $taskNativePath) }
        python @taskPythonArguments
        if ($LASTEXITCODE -ne 0) { throw 'Independent differential verification failed' }
    }
} finally { $taskSlot.Dispose() }
