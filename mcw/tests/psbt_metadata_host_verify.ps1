param([string]$NativeApplication, [switch]$BuildOnly, [ValidateRange(0,300)][int]$WaitForSlotSeconds = 0)
$ErrorActionPreference = 'Stop'
$metadataRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$metadataCommon = & git -C $metadataRoot rev-parse --path-format=absolute --git-common-dir
if ($LASTEXITCODE -ne 0) { throw 'Cannot locate shared root.' }
$metadataShared = Split-Path -Parent ($metadataCommon | Select-Object -First 1)
$metadataRun = (Get-Date -Format 'yyyyMMdd-HHmmss-fff') + '-' + [guid]::NewGuid().ToString('N').Substring(0,8)
$metadataOutput = Join-Path $metadataRoot ".artifacts/psbt-metadata-host/$metadataRun"
New-Item -ItemType Directory -Path $metadataOutput -Force | Out-Null
$metadataProject = Join-Path $metadataOutput 'metadata-child.csproj'
$metadataCoreProject = [Security.SecurityElement]::Escape((Join-Path $metadataRoot 'MagicalCryptoWallet/MagicalCryptoWallet.csproj'))
$metadataHostSource = [Security.SecurityElement]::Escape((Join-Path $metadataRoot 'MagicalCryptoWallet.Client/Application/ManagedApplicationHost.cs'))
$metadataTestSource = [Security.SecurityElement]::Escape((Join-Path $PSScriptRoot 'psbt_metadata_host_reference.cs'))
$metadataProjectText = @"
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup><OutputType>Exe</OutputType><AssemblyName>magicalcryptowallet</AssemblyName><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><EnableDefaultCompileItems>false</EnableDefaultCompileItems><IsPackable>false</IsPackable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup>
  <ItemGroup><ProjectReference Include="$metadataCoreProject" /><Compile Include="$metadataHostSource" Link="ManagedApplicationHost.cs" /><Compile Include="$metadataTestSource" Link="MetadataReference.cs" /></ItemGroup>
</Project>
"@
[IO.File]::WriteAllText($metadataProject, $metadataProjectText, [Text.UTF8Encoding]::new($false))
$metadataSlot = $null
$metadataSlotDeadline = [DateTime]::UtcNow.AddSeconds($WaitForSlotSeconds)
do {
    foreach ($metadataIndex in 1..2) {
        try {
            $metadataSlot = [IO.File]::Open((Join-Path $metadataShared ".artifacts/mcw-coordination/build-slot-$metadataIndex.lock"), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
            break
        } catch [IO.IOException] { }
    }
    if ($null -ne $metadataSlot -or [DateTime]::UtcNow -ge $metadataSlotDeadline) { break }
    Start-Sleep -Seconds 2
} while ($true)
if ($null -eq $metadataSlot) { throw 'Both build slots occupied; host-test project preserved.' }
try {
    Write-Output "BUILD_SLOT=$metadataIndex VERIFIER_PID=$PID ASSIGNMENT=psbt_metadata_managed_host"
    if ((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory * 1KB -lt 2GB) { throw 'Less than 2 GiB free RAM; defer compilation.' }
    # Existing SDK and retained package graph only; no new production dependency.
    & dotnet build $metadataProject -c Release -m:1 /p:UseSharedCompilation=false 2>&1 | Tee-Object -FilePath (Join-Path $metadataOutput 'managed-build.log')
    if ($LASTEXITCODE -ne 0) { throw 'Actual managed core/metadata caller build failed.' }
    $metadataEvidence = [ordered]@{
        core_build='passed';factory_sha256=(Get-FileHash (Join-Path $metadataRoot 'MagicalCryptoWallet/Blockchain/Transactions/TransactionFactory.cs')).Hash.ToLowerInvariant();adapter_sha256=(Get-FileHash (Join-Path $metadataRoot 'MagicalCryptoWallet/Mcw/Psbt/McwPsbtMetadata.cs')).Hash.ToLowerInvariant();host_source_sha256=(Get-FileHash (Join-Path $metadataRoot 'MagicalCryptoWallet.Client/Application/ManagedApplicationHost.cs')).Hash.ToLowerInvariant();native_host_verified=$false;product_release=$false
    }
    if (-not $BuildOnly) {
        if (-not $NativeApplication -or -not (Test-Path -LiteralPath $NativeApplication -PathType Leaf)) { throw 'A freshly integrated actual mcw application is required.' }
        $metadataChildOutput = Join-Path $metadataOutput 'bin/Release/net10.0'
        $metadataNative = Join-Path $metadataChildOutput 'mcw.exe'
        Copy-Item -LiteralPath $NativeApplication -Destination $metadataNative
        & $metadataNative gui 2>&1 | Tee-Object -FilePath (Join-Path $metadataOutput 'actual-host.log')
        if ($LASTEXITCODE -ne 0) { throw 'Actual Rust host/managed metadata/factory validation failed.' }
        if (-not (Select-String -LiteralPath (Join-Path $metadataOutput 'actual-host.log') -SimpleMatch 'MCW_PSBT_HOST_VERIFIED metadata=2 factory=2 signing=retained packets=synthetic')) { throw 'Missing actual-path verification marker.' }
        $metadataEvidence.native_host_verified = $true
        $metadataEvidence['native_sha256'] = (Get-FileHash -LiteralPath $metadataNative).Hash.ToLowerInvariant()
    }
    $metadataEvidence | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $metadataOutput 'evidence.json') -Encoding utf8
    Write-Output "HOST_METADATA_EVIDENCE=$metadataOutput"
} finally { $metadataSlot.Dispose() }
