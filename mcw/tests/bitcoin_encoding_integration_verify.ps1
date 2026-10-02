param(
    [Parameter(Mandatory)][string]$IntegrationRoot,
    [Parameter(Mandatory)][string]$RustToolchain,
    [Parameter(Mandatory)][string]$SharedProject,
    [string]$EvidenceDirectory = ''
)
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'This bounded native integration verifier requires Windows.' }
$taskRoot = [IO.Path]::GetFullPath($IntegrationRoot)
$taskShared = [IO.Path]::GetFullPath($SharedProject)
$taskTools = [IO.Path]::GetFullPath($RustToolchain)
if (-not $EvidenceDirectory) { $EvidenceDirectory = Join-Path $taskRoot '.artifacts/bitcoin-address-evidence' }
$taskEvidence = [IO.Path]::GetFullPath($EvidenceDirectory)
New-Item -ItemType Directory -Force -Path $taskEvidence | Out-Null
$taskSlot = $null
foreach ($taskNumber in 1,2) {
    try { $taskSlot = [IO.File]::Open((Join-Path $taskShared ".artifacts/mcw-coordination/build-slot-$taskNumber.lock"), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None); break }
    catch [IO.IOException] { }
}
if ($null -eq $taskSlot) { Write-Output 'BUILD_SLOTS_BUSY'; exit 3 }
$taskVariables = @('PATH','CARGO_BUILD_JOBS','CARGO_TARGET_DIR','CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER','CARGO_ENCODED_RUSTFLAGS','RUSTFLAGS','RUSTUP_HOME','CARGO_HOME')
$taskPrevious = @{}
foreach ($taskVariable in $taskVariables) { $taskPrevious[$taskVariable] = [Environment]::GetEnvironmentVariable($taskVariable, 'Process') }
try {
    if ((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory -lt 2MB) { Write-Output 'LOW_MEMORY'; exit 3 }
    $taskCargo = Join-Path $taskTools 'cargo.exe'
    $taskRustc = Join-Path $taskTools 'rustc.exe'
    $taskManifest = Join-Path $taskRoot 'mcw/Cargo.toml'
    $taskVersion = & $taskRustc --version
    if ($LASTEXITCODE -or $taskVersion -notmatch '^rustc 1\.99\.0 ') { throw 'Rust version must be 1.99.0.' }
    $env:PATH = "$taskTools;$env:PATH"
    $env:CARGO_BUILD_JOBS = '1'
    $env:CARGO_TARGET_DIR = Join-Path $taskEvidence 'cargo-test'
    $env:RUSTUP_HOME = Join-Path $taskShared '.artifacts/mcw-tools/rustup'
    $env:CARGO_HOME = Join-Path $taskShared '.artifacts/mcw-tools/cargo'
    $taskVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $taskVsRoot = & $taskVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    $taskMsvc = Get-ChildItem -LiteralPath (Join-Path $taskVsRoot 'VC/Tools/MSVC') -Directory | Sort-Object Name -Descending | Select-Object -First 1
    $taskSdk = Get-ChildItem -LiteralPath (Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/Lib') -Directory | Sort-Object Name -Descending | Select-Object -First 1
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = Join-Path $taskMsvc.FullName 'bin/Hostx64/x64/link.exe'
    $env:RUSTFLAGS = $null
    $env:CARGO_ENCODED_RUSTFLAGS = @('-C','target-feature=+crt-static','-L',('native=' + (Join-Path $taskMsvc.FullName 'lib/onecore/x64')),'-L',('native=' + (Join-Path $taskSdk.FullName 'ucrt/x64')),'-L',('native=' + (Join-Path $taskSdk.FullName 'um/x64'))) -join [char]31
    $taskMetadata = & $taskCargo metadata --manifest-path $taskManifest --locked --offline --format-version 1
    if ($LASTEXITCODE) { throw 'Cargo metadata failed.' }
    $taskMetadata | Set-Content -LiteralPath (Join-Path $taskEvidence 'cargo-metadata.json')
    $taskGraph = ($taskMetadata -join "`n") | ConvertFrom-Json
    if ($taskGraph.packages.Count -ne 1 -or $taskGraph.packages[0].dependencies.Count) { throw 'External Cargo dependency or additional package found.' }
    & (Join-Path $taskTools 'rustfmt.exe') --edition 2024 --check --config skip_children=true (Join-Path $taskRoot 'mcw/src/lib.rs') (Join-Path $taskRoot 'mcw/src/app.rs') (Join-Path $taskRoot 'mcw/src/bitcoin_encoding/address_service.rs') (Join-Path $taskRoot 'mcw/tests/bitcoin_encoding_service.rs')
    if ($LASTEXITCODE) { throw 'Deferred native integration formatting failed.' }
    foreach ($taskProfile in 'debug','release') {
        $taskArguments = @('test','--manifest-path',$taskManifest,'--locked','--offline','--test','bitcoin_encoding_service')
        if ($taskProfile -eq 'release') { $taskArguments += '--release' }
        & $taskCargo @taskArguments 2>&1 | Tee-Object -FilePath (Join-Path $taskEvidence "handler-$taskProfile.log")
        if ($LASTEXITCODE) { throw "Native handler $taskProfile test failed." }
    }
    & $taskCargo clippy --manifest-path $taskManifest --locked --offline --test bitcoin_encoding_service -- -D warnings 2>&1 | Tee-Object -FilePath (Join-Path $taskEvidence 'handler-clippy.log')
    if ($LASTEXITCODE) { throw 'Native handler Clippy failed.' }
    & $taskCargo build --manifest-path $taskManifest --locked --offline --release --bin mcw 2>&1 | Tee-Object -FilePath (Join-Path $taskEvidence 'host-build.log')
    if ($LASTEXITCODE) { throw 'Actual native host build failed.' }
    $taskBinary = Join-Path $env:CARGO_TARGET_DIR 'release/mcw.exe'
    $taskDumpbin = Join-Path $taskMsvc.FullName 'bin/Hostx64/x64/dumpbin.exe'
    $taskImports = & $taskDumpbin /nologo /imports $taskBinary
    if ($LASTEXITCODE) { throw 'PE import read failed.' }
    $taskImports | Set-Content -LiteralPath (Join-Path $taskEvidence 'host-imports.txt')
    if (($taskImports -join "`n") -match '(?i)(vcruntime|msvcp\d|msvcr\d|libgcc|libstdc\+\+|libc\+\+)') { throw 'Non-OS dynamic runtime import found.' }

    $taskProbe = Join-Path $taskEvidence 'managed-probe'
    New-Item -ItemType Directory -Force -Path $taskProbe | Out-Null
    # This temporary test project has no PackageReference and is never shipped.
    '<Project />' | Set-Content -LiteralPath (Join-Path $taskProbe 'Directory.Build.props')
    $taskCoreProject = [Security.SecurityElement]::Escape((Join-Path $taskRoot 'MagicalCryptoWallet/MagicalCryptoWallet.csproj'))
    $taskManagedHost = [Security.SecurityElement]::Escape((Join-Path $taskRoot 'MagicalCryptoWallet.Client/Application/ManagedApplicationHost.cs'))
    $taskProject = @"
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup><TargetFramework>net10.0</TargetFramework><OutputType>Exe</OutputType><AssemblyName>magicalcryptowalletd</AssemblyName><LangVersion>14</LangVersion><Nullable>enable</Nullable><BuildMcwHost>false</BuildMcwHost><NuGetAudit>false</NuGetAudit></PropertyGroup>
  <ItemGroup><ProjectReference Include="$taskCoreProject" /><Compile Include="$taskManagedHost" Link="ManagedApplicationHost.cs" /></ItemGroup>
</Project>
"@
    $taskProject | Set-Content -LiteralPath (Join-Path $taskProbe 'BitcoinAddressProbe.csproj')
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'bitcoin_encoding_managed_probe.cs') -Destination (Join-Path $taskProbe 'Program.cs')
    $taskOutput = Join-Path $taskProbe 'output'
    & dotnet build (Join-Path $taskProbe 'BitcoinAddressProbe.csproj') -c Release -o $taskOutput -m:1 -p:BuildInParallel=false -p:BuildMcwHost=false -p:NuGetAudit=false --ignore-failed-sources 2>&1 | Tee-Object -FilePath (Join-Path $taskEvidence 'managed-build.log')
    if ($LASTEXITCODE) { throw 'Actual managed caller/host test build failed.' }
    Copy-Item -LiteralPath $taskBinary -Destination (Join-Path $taskOutput 'mcw.exe')
    $taskReport = Join-Path $taskEvidence 'managed-results.json'
    if (Test-Path -LiteralPath $taskReport) { throw 'Use a fresh evidence directory; do not reuse a previous success report.' }
    $taskNativeHost = Join-Path $taskOutput 'mcw.exe'
    & $taskNativeHost daemon (Join-Path $taskRoot 'mcw/tests/bitcoin_encoding_fixtures') $taskReport 2>&1 | Tee-Object -FilePath (Join-Path $taskEvidence 'managed-run.log')
    if ($LASTEXITCODE -or -not (Test-Path -LiteralPath $taskReport)) { throw 'Actual host/caller integration did not produce a successful report.' }
    $taskResult = Get-Content -LiteralPath $taskReport -Raw | ConvertFrom-Json
    if ($taskResult.core_valid -ne 54 -or $taskResult.core_invalid -ne 70 -or $taskResult.concurrent_requests -ne 64 -or $taskResult.dependency_removed) { throw 'Integration report scope/count mismatch.' }
    [ordered]@{ native_target = 'x86_64-pc-windows-msvc'; rust_version = $taskVersion; integration_base = (& git -C $taskRoot rev-parse HEAD); host_sha256 = (Get-FileHash -LiteralPath $taskBinary -Algorithm SHA256).Hash.ToLowerInvariant(); source_sha256 = (Get-FileHash -LiteralPath (Join-Path $taskRoot 'mcw/src/bitcoin_encoding/address_service.rs') -Algorithm SHA256).Hash.ToLowerInvariant(); result = 'pass'; five_target_release_verified = $false; dependency_removed = $false } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskEvidence 'verification.json')
    Get-Content -LiteralPath $taskReport
} finally {
    foreach ($taskVariable in $taskVariables) { [Environment]::SetEnvironmentVariable($taskVariable, $taskPrevious[$taskVariable], 'Process') }
    $taskSlot.Dispose()
}
