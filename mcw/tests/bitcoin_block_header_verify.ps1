param(
    [string]$SharedRoot = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',
    [string]$HostSourceRoot = '',
    [string]$Python = 'C:\Python314\python.exe'
)
$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if (-not $HostSourceRoot) { $HostSourceRoot = $repoRoot }
$evidenceRoot = Join-Path $repoRoot '.artifacts/bitcoin-block-header-evidence'
$runRoot = Join-Path $evidenceRoot ('run-' + [Guid]::NewGuid().ToString('N'))
$nativeRoot = Join-Path $runRoot 'native'
$managedRoot = Join-Path $runRoot 'managed'
$applicationRoot = Join-Path $runRoot 'application'
$toolBin = Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin'
$buildHandle = $null
$sourceRecords = [Collections.Generic.List[object]]::new()
function File-Hash([string]$path) { (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Freeze-Source([string]$source, [string]$destination) {
    $data = [IO.File]::ReadAllBytes($source)
    New-Item -ItemType Directory -Force -Path (Split-Path $destination -Parent) | Out-Null
    [IO.File]::WriteAllBytes($destination, $data)
    $sourceRecords.Add([ordered]@{source=$source;snapshot=$destination;sha256=([Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($data)).ToLowerInvariant())})
}
function Xml-Path([string]$path) { [Security.SecurityElement]::Escape([IO.Path]::GetFullPath($path)) }
function Require-Success([string]$label) { if ($LASTEXITCODE -ne 0) { throw "$label failed ($LASTEXITCODE). Evidence remains in $runRoot" } }
try {
    if ((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory -lt 2097152) { Write-Output 'Build deferred: less than 2 GiB free memory'; exit 75 }
    foreach ($slot in 1, 2) {
        try { $buildHandle = [IO.File]::Open((Join-Path $SharedRoot ('.artifacts/mcw-coordination/build-slot-' + $slot + '.lock')), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None); Write-Output "Build slot $slot acquired by verifier PID $PID for existing block-cache header assignment"; break }
        catch [IO.IOException] { }
    }
    if (-not $buildHandle) { Write-Output 'Build deferred: both compiler slots occupied'; exit 75 }
    New-Item -ItemType Directory -Force -Path $nativeRoot, $managedRoot, $applicationRoot | Out-Null
    foreach ($file in Get-ChildItem -LiteralPath (Join-Path $HostSourceRoot 'mcw/src') -Recurse -File) {
        $relative = [IO.Path]::GetRelativePath((Join-Path $HostSourceRoot 'mcw/src'), $file.FullName)
        Freeze-Source $file.FullName (Join-Path $nativeRoot ('src/' + $relative))
    }
    foreach ($name in 'Cargo.toml', 'Cargo.lock', 'build.rs') { Freeze-Source (Join-Path $HostSourceRoot ('mcw/' + $name)) (Join-Path $nativeRoot $name) }
    Freeze-Source (Join-Path $repoRoot 'mcw/src/bitcoin_block_service.rs') (Join-Path $nativeRoot 'src/bitcoin_block_service.rs')
    $libraryPath = Join-Path $nativeRoot 'src/lib.rs'
    $appPath = Join-Path $nativeRoot 'src/app.rs'
    $library = [IO.File]::ReadAllText($libraryPath)
    $app = [IO.File]::ReadAllText($appPath)
    $proposedSharedPatch = -not $library.Contains('pub mod bitcoin_block_service;')
    if ($proposedSharedPatch) {
        if (-not $library.Contains('pub mod bitcoin_block;')) { throw 'Real host block registration is missing.' }
        $library = $library.Replace('pub mod bitcoin_block;', 'pub mod bitcoin_block;' + "`n" + 'pub mod bitcoin_block_service;')
        [IO.File]::WriteAllText($libraryPath, $library)
    }
    if (-not $app.Contains('bitcoin_block_service::hash_header')) {
        $anchor = '        bridge::QR => bridge::encode_qr(frame).write(output),'
        if (-not $app.Contains($anchor)) { throw 'Shared dispatch changed; request an updated patch from its owner.' }
        $leaf = @'
        crate::bitcoin_block_service::HASH_HEADER => {
            match crate::bitcoin_block_service::hash_header(&frame.payload) {
                Ok(digest) => frame.reply(digest.to_vec()).write(output),
                Err(_) => frame.error(1, "invalid block header").write(output),
            }
        }
'@
        $app = $app.Replace($anchor, $anchor + "`n" + $leaf)
        [IO.File]::WriteAllText($appPath, $app)
        $proposedSharedPatch = $true
    }
    $api = Join-Path $managedRoot 'IMcwApplicationServices.cs'
    $managedHostSource = Join-Path $managedRoot 'ManagedApplicationHost.cs'
    Freeze-Source (Join-Path $HostSourceRoot 'MagicalCryptoWallet/Mcw/IMcwApplicationServices.cs') $api
    Freeze-Source (Join-Path $HostSourceRoot 'MagicalCryptoWallet.Client/Application/ManagedApplicationHost.cs') $managedHostSource
    $coreProject = Join-Path $repoRoot 'MagicalCryptoWallet/MagicalCryptoWallet.csproj'
    $probeProject = Join-Path $managedRoot 'BlockHeaderProbe.csproj'
    $injection = Join-Path $managedRoot 'actual-host-contract.targets'
    $targets = @'
<Project><ItemGroup Condition="'$(MSBuildProjectName)' == 'MagicalCryptoWallet'">
<Compile Include="@API@" Link="Mcw/IMcwApplicationServices.cs" Condition="!Exists('$(MSBuildProjectDirectory)/Mcw/IMcwApplicationServices.cs')" />
</ItemGroup></Project>
'@
    [IO.File]::WriteAllText($injection, $targets.Replace('@API@', (Xml-Path $api)))
    $nuget = Join-Path $env:USERPROFILE '.nuget/packages'
    $project = @'
<Project Sdk="Microsoft.NET.Sdk">
<PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><EnableDefaultCompileItems>false</EnableDefaultCompileItems><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><RestorePackagesWithLockFile>false</RestorePackagesWithLockFile></PropertyGroup>
<ItemGroup>
<ProjectReference Include="@CORE@" />
<Compile Include="@HOST@" Link="ManagedApplicationHost.cs" />
<Compile Include="@PROBE@" Link="Program.cs" />
<Compile Include="@TESTS@" Link="FileSystemBlockRepositoryTests.cs" />
<Reference Include="xunit.v3.assert"><HintPath>@ASSERT@</HintPath></Reference>
<Reference Include="xunit.v3.core"><HintPath>@XCORE@</HintPath></Reference>
</ItemGroup></Project>
'@
    foreach ($pair in @(
        @('@CORE@', $coreProject), @('@HOST@', $managedHostSource),
        @('@PROBE@', (Join-Path $PSScriptRoot 'bitcoin_block_header_probe.cs')),
        @('@TESTS@', (Join-Path $repoRoot 'MagicalCryptoWallet.Tests/UnitTests/Wallet/FileSystemBlockRepositoryTests.cs')),
        @('@ASSERT@', (Join-Path $nuget 'xunit.v3.assert/3.2.2/lib/net8.0/xunit.v3.assert.dll')),
        @('@XCORE@', (Join-Path $nuget 'xunit.v3.extensibility.core/3.2.2/lib/netstandard2.0/xunit.v3.core.dll'))
    )) { $project = $project.Replace($pair[0], (Xml-Path $pair[1])) }
    [IO.File]::WriteAllText($probeProject, $project)
    $linker = Get-ChildItem -Path 'C:\Program Files\Microsoft Visual Studio\*\*\VC\Tools\MSVC\*\bin\Hostx64\x64\link.exe' | Sort-Object FullName -Descending | Select-Object -First 1
    $msvc = [IO.Path]::GetFullPath((Join-Path $linker.Directory.FullName '../../..'))
    $sdk = Get-ChildItem -LiteralPath 'C:\Program Files (x86)\Windows Kits\10\Lib' -Directory | Sort-Object Name -Descending | Select-Object -First 1
    $env:PATH = $toolBin + ';' + $linker.Directory.FullName + ';' + $env:PATH
    $env:LIB = (Join-Path $msvc 'lib/onecore/x64') + ';' + (Join-Path $sdk.FullName 'ucrt/x64') + ';' + (Join-Path $sdk.FullName 'um/x64')
    $env:CARGO_BUILD_JOBS = '1'
    $env:RUSTFLAGS = '-D warnings -C target-feature=+crt-static -C codegen-units=1 -C overflow-checks=yes'
    # MSVC's build-script executable path must remain below its legacy limit.
    # Keep unique owned compiler outputs short; source/evidence stay in runRoot.
    $env:CARGO_TARGET_DIR = Join-Path $SharedRoot ('.artifacts/bh-build/' + [Guid]::NewGuid().ToString('N').Substring(0, 8))
    $env:MSBUILDDISABLENODEREUSE = '1'
    $compiler = & (Join-Path $toolBin 'rustc.exe') --version
    Require-Success 'Rust version'
    if ($compiler -notlike 'rustc 1.99.0 *') { throw 'Rust 1.99.0 is required.' }
    Write-Output $compiler
    $metadata = & (Join-Path $toolBin 'cargo.exe') metadata --manifest-path (Join-Path $nativeRoot 'Cargo.toml') --offline --locked --format-version 1
    Require-Success 'Actual host metadata'
    $graph = $metadata | ConvertFrom-Json
    if ($graph.packages.Count -ne 1 -or $graph.packages[0].dependencies.Count -ne 0) { throw 'One first-party mcw package with zero external crates is required.' }
    & (Join-Path $toolBin 'cargo.exe') clippy --manifest-path (Join-Path $nativeRoot 'Cargo.toml') --lib --bin mcw --offline --locked -- -D warnings 2>&1 | Tee-Object -FilePath (Join-Path $runRoot 'native-clippy.log')
    Require-Success 'Actual host Clippy'
    & (Join-Path $toolBin 'cargo.exe') build --manifest-path (Join-Path $nativeRoot 'Cargo.toml') --bin mcw --offline --locked -j 1 2>&1 | Tee-Object -FilePath (Join-Path $runRoot 'native-build.log')
    Require-Success 'Actual mcw host build'
    & dotnet restore $probeProject --locked-mode --source $nuget -p:NuGetAudit=false -p:CustomAfterMicrosoftCommonTargets=$injection -m:1 2>&1 | Tee-Object -FilePath (Join-Path $runRoot 'managed-restore.log')
    Require-Success 'Cached managed restore'
    & dotnet build $probeProject --no-restore -c Release -m:1 -p:UseSharedCompilation=false -p:CustomAfterMicrosoftCommonTargets=$injection 2>&1 | Tee-Object -FilePath (Join-Path $runRoot 'managed-build.log')
    Require-Success 'Actual managed caller/host build'
    Copy-Item -Path (Join-Path $managedRoot 'bin/Release/net10.0/*') -Destination $applicationRoot -Recurse
    Copy-Item -LiteralPath (Join-Path $applicationRoot 'BlockHeaderProbe.exe') -Destination (Join-Path $applicationRoot 'magicalcryptowallet.exe')
    $binary = Join-Path $applicationRoot 'mcw.exe'
    Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'debug/mcw.exe') -Destination $binary
    $fixture = Join-Path $PSScriptRoot 'bitcoin_block_fixtures/headers.tsv'
    $cache = Join-Path $runRoot 'synthetic-cache'
    $reports = [ordered]@{}
    foreach ($action in 'verify', 'recover') {
        $report = Join-Path $runRoot ($action + '.json')
        & $binary gui $action $report $fixture $cache 2>&1 | Tee-Object -FilePath (Join-Path $runRoot ($action + '.log'))
        Require-Success ('Real host cache ' + $action)
        $reports[$action] = Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
    }
    $dumpbin = Join-Path $linker.Directory.FullName 'dumpbin.exe'
    & $dumpbin /dependents $binary | Set-Content -LiteralPath (Join-Path $runRoot 'mcw-imports.txt') -Encoding utf8
    Require-Success 'Native host runtime imports'
    $sourceRecords.Add([ordered]@{source='proposed shared lib.rs patch';snapshot=$libraryPath;sha256=(File-Hash $libraryPath)})
    $sourceRecords.Add([ordered]@{source='proposed shared app.rs dispatch patch';snapshot=$appPath;sha256=(File-Hash $appPath)})
    $hostRevision = & git -C $HostSourceRoot rev-parse HEAD
    Require-Success 'Host source revision'
    $result = [ordered]@{state='pass';scope='80-byte cache header hash and identity only';run=$runRoot;real_host=$true;compiler=$compiler;host_source_revision=$hostRevision;shared_patch_applied_to_snapshot=$proposedSharedPatch;production_incorporation_verified=$false;publication_note='Verified separately against remote master after atomic publication.';synthetic_only=$true;external_cargo_dependencies=0;cargo_packages=1;mcw_sha256=(File-Hash $binary);caller_sha256=(File-Hash (Join-Path $repoRoot 'MagicalCryptoWallet/Wallets/FileSystemBlockRepository.cs'));adapter_sha256=(File-Hash (Join-Path $repoRoot 'MagicalCryptoWallet/Mcw/Blocks/McwBlockHeaderService.cs'));fixture_sha256=(File-Hash $fixture);reports=$reports;sources=$sourceRecords;platform='Windows x64';other_native_platforms_verified=$false;production_release=$false}
    $result | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $evidenceRoot 'real-host-verification.json') -Encoding utf8
    [pscustomobject]$result | Select-Object state, run, real_host, shared_patch_applied_to_snapshot, production_incorporation_verified, reports | ConvertTo-Json -Depth 5
}
finally { if ($buildHandle) { $buildHandle.Dispose() } }
