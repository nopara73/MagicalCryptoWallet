param(
    [string]$SharedRoot = 'C:/Users/user/OneDrive/Documents/ChatGPT/MagicalCryptoWallet',
    [string]$NativeTargetDir
)
$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$evidenceRoot = Join-Path $repoRoot '.artifacts/script-text-client-evidence'
$toolsBin = Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin'
$cargo = Join-Path $toolsBin 'cargo.exe'
$project = Join-Path $PSScriptRoot 'script_text_client/ClientCheck.csproj'
$buildHandle = $null
$variables = @('PATH', 'CARGO_HOME', 'CARGO_BUILD_JOBS', 'CARGO_TARGET_DIR')
$previous = @{}
foreach ($variable in $variables) { $previous[$variable] = [Environment]::GetEnvironmentVariable($variable, 'Process') }
New-Item -ItemType Directory -Force -Path $evidenceRoot | Out-Null
try {
    foreach ($number in 1, 2) {
        try {
            $buildHandle = [IO.File]::Open((Join-Path $SharedRoot ".artifacts/mcw-coordination/build-slot-$number.lock"), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
            break
        } catch [IO.IOException] { }
    }
    if ($null -eq $buildHandle) { Write-Output 'BUILD_SLOTS_BUSY'; exit 3 }
    Write-Output "BUILD_SLOT=$number VERIFIER_PID=$PID ASSIGNMENT=script-text-real-host-caller-check"
    if ((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory -lt 2097152) { throw 'At least 2 GiB free RAM required.' }
    if (-not (Test-Path -LiteralPath $cargo)) { throw 'Existing Rust 1.99.0 toolchain required.' }
    $env:PATH = $toolsBin + ';' + $env:PATH
    $env:CARGO_HOME = Join-Path $SharedRoot '.artifacts/mcw-tools/cargo'
    $env:CARGO_BUILD_JOBS = '1'
    $env:CARGO_TARGET_DIR = if ($NativeTargetDir) { [IO.Path]::GetFullPath($NativeTargetDir) } else { Join-Path $evidenceRoot 'native-target' }
    # Build the actual single host, with the established std/native-OS runtime.
    # The caller/dispatcher source must already be incorporated in this checkout.
    & (Join-Path $repoRoot 'Contrib/Mcw/build-windows.ps1') -Cargo $cargo -Version '99.99.99' 2>&1 | Tee-Object -FilePath (Join-Path $evidenceRoot 'native-build.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Actual mcw host build failed.' }
    $binary = Join-Path $env:CARGO_TARGET_DIR 'x86_64-pc-windows-msvc/release/mcw.exe'
    & dumpbin.exe /nologo /imports $binary | Set-Content -LiteralPath (Join-Path $evidenceRoot 'native-imports.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Actual host import audit failed.' }
    & dotnet build $project --disable-build-servers --nologo -m:1 -p:UseSharedCompilation=false -v:minimal 2>&1 | Tee-Object -FilePath (Join-Path $evidenceRoot 'build.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Actual production managed caller build failed.' }
    $managedOutput = Join-Path $PSScriptRoot 'script_text_client/bin/Debug/net10.0'
    & dotnet (Join-Path $managedOutput 'ClientCheck.dll') 2>&1 | Tee-Object -FilePath (Join-Path $evidenceRoot 'routing.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Client/coordinator routing checks failed.' }
    $runtime = Join-Path $evidenceRoot 'synthetic-runtime'
    New-Item -ItemType Directory -Force -Path $runtime | Out-Null
    Get-ChildItem -LiteralPath $managedOutput | Copy-Item -Destination $runtime -Recurse -Force
    Copy-Item -LiteralPath $binary -Destination (Join-Path $runtime 'mcw.exe') -Force
    # A development apphost supplies the managed role; its embedded DLL remains
    # ClientCheck.dll. No wallet executable, user state or network is involved.
    Copy-Item -LiteralPath (Join-Path $managedOutput 'ClientCheck.exe') -Destination (Join-Path $runtime 'magicalcryptowallet.exe') -Force
    $stdout = Join-Path $evidenceRoot 'native-caller-stdout.txt'
    $stderr = Join-Path $evidenceRoot 'native-caller-stderr.txt'
    $process = Start-Process -FilePath (Join-Path $runtime 'mcw.exe') -ArgumentList @('gui', '--script-text-native-child') -WorkingDirectory $runtime -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $deadline = [DateTime]::UtcNow.AddSeconds(90)
    while (-not $process.WaitForExit(1000)) {
        if ([DateTime]::UtcNow -ge $deadline) { $process.Kill($true); throw 'Owned synthetic mcw caller probe timed out.' }
    }
    $process.WaitForExit()
    $nativeLog = Get-Content -Raw -LiteralPath $stderr
    Write-Output $nativeLog
    if ($process.ExitCode -ne 0 -or $nativeLog -notmatch 'SCRIPT_TEXT_NATIVE_CALLER_CHECKS=(\d+)') { throw 'Real native caller check failed.' }
    $nativeChecks = [int]$Matches[1]
    $routingLog = Get-Content -Raw -LiteralPath (Join-Path $evidenceRoot 'routing.txt')
    if ($routingLog -notmatch 'SCRIPT_TEXT_CLIENT_ROUTING_CHECKS=(\d+)') { throw 'Routing check evidence missing.' }
    $record = [ordered]@{
        base_commit = (& git -C $repoRoot rev-parse HEAD)
        source_root = $repoRoot
        native_binary_sha256 = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant()
        managed_build_succeeded = $true
        routing_checks = [int]$Matches[1]
        native_caller_checks = $nativeChecks
        native_exit_code = $process.ExitCode
        synthetic_only = $true
        shipping_artifact = $false
        native_platform = 'Windows x64'
        note = 'Actual retained production Core/Client plus actual mcw host and application service connection. HTTP is in-memory. Other platform/release acceptance remains separate.'
    }
    $record | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $evidenceRoot 'verification.json')
} finally {
    if ($buildHandle) { $buildHandle.Dispose() }
    foreach ($variable in $variables) { [Environment]::SetEnvironmentVariable($variable, $previous[$variable], 'Process') }
}
