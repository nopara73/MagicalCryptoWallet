# Prepare test integration for shared-owner review; do not edit the live suite.
param([string]$SourceRoot)
$ErrorActionPreference = 'Stop'
$metadataRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$metadataSource = if ($SourceRoot) { [IO.Path]::GetFullPath($SourceRoot) } else { $metadataRoot }
$metadataPatchRoot = Join-Path $metadataRoot '.artifacts/psbt-metadata-suite-patch'
New-Item -ItemType Directory -Path $metadataPatchRoot -Force | Out-Null
$metadataProject = [IO.File]::ReadAllText((Join-Path $metadataSource 'MagicalCryptoWallet.Tests/MagicalCryptoWallet.Tests.csproj')).Replace("`r`n", "`n")
$metadataInitializer = [IO.File]::ReadAllText((Join-Path $metadataSource 'MagicalCryptoWallet.Tests/ModuleInitializer.cs')).Replace("`r`n", "`n")
$metadataHost = [IO.File]::ReadAllText((Join-Path $metadataSource 'MagicalCryptoWallet.Client/Application/ManagedApplicationHost.cs')).Replace("`r`n", "`n")
if ($metadataProject.Contains('psbt_metadata_managed_fixture') -or $metadataInitializer.Contains('McwManagedTestHost.Initialize()')) { throw 'Suite integration already changed; review before preparing a patch.' }
$metadataFixtureGroup = @'
	<ItemGroup>
		<ProjectReference Include="..\MagicalCryptoWallet.Client\MagicalCryptoWallet.Client.csproj" />
		<Compile Include="..\mcw\tests\psbt_metadata_managed_fixture.cs" Link="Infrastructure\McwManagedTestHost.cs" />
	</ItemGroup>

'@
$metadataProject = $metadataProject.Replace('</Project>', $metadataFixtureGroup + "`n</Project>")
$metadataProject = $metadataProject.Replace('<CopyLocalLockFileAssemblies>true</CopyLocalLockFileAssemblies>', '<CopyLocalLockFileAssemblies>true</CopyLocalLockFileAssemblies>' + "`n`t`t<AllowUnsafeBlocks>true</AllowUnsafeBlocks>")
$metadataInitializer = $metadataInitializer.Replace("internal static void Initialize()`n`t{", "internal static void Initialize()`n`t{`n`t`tglobal::MagicalCryptoWallet.Tests.Infrastructure.McwManagedTestHost.Initialize();`n")
if (-not $metadataInitializer.Contains('McwManagedTestHost.Initialize();')) { throw 'Test startup shape changed; review the fixture hook.' }
[IO.File]::WriteAllText((Join-Path $metadataPatchRoot 'MagicalCryptoWallet.Tests.csproj'), $metadataProject, [Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText((Join-Path $metadataPatchRoot 'ModuleInitializer.cs'), $metadataInitializer, [Text.UTF8Encoding]::new($false))
$metadataHostOriginal = @'
    public static ManagedApplicationHost Connect()
    {
        if (Current is not null) { throw new InvalidOperationException("The application already has a host connection."); }
        var host = new ManagedApplicationHost(Console.OpenStandardInput(), Console.OpenStandardOutput());
'@
$metadataHostReplacement = @'
    public static ManagedApplicationHost Connect()
    {
        if (Current is not null) { throw new InvalidOperationException("The application already has a host connection."); }
        return Connect(Console.OpenStandardInput(), Console.OpenStandardOutput());
    }

    /// <summary>Connect using owned protocol streams, including an isolated test runner's reserved pipe.</summary>
    public static ManagedApplicationHost Connect(Stream input, Stream output)
    {
        ArgumentNullException.ThrowIfNull(input);
        ArgumentNullException.ThrowIfNull(output);
        if (Current is not null) { throw new InvalidOperationException("The application already has a host connection."); }
        var host = new ManagedApplicationHost(input, output);
'@
if (-not $metadataHost.Contains($metadataHostOriginal)) { throw 'Managed host connection shape changed; review the owned-stream overload.' }
$metadataHost = $metadataHost.Replace($metadataHostOriginal, $metadataHostReplacement)
[IO.File]::WriteAllText((Join-Path $metadataPatchRoot 'ManagedApplicationHost.cs'), $metadataHost, [Text.UTF8Encoding]::new($false))
$metadataIntegrationProject = [IO.File]::ReadAllText((Join-Path $metadataSource 'MagicalCryptoWallet.IntegrationTests/MagicalCryptoWallet.IntegrationTests.csproj')).Replace("`r`n", "`n")
$metadataIntegrationInitializer = [IO.File]::ReadAllText((Join-Path $metadataSource 'MagicalCryptoWallet.IntegrationTests/ModuleInitializer.cs')).Replace("`r`n", "`n")
if ($metadataIntegrationProject.Contains('psbt_metadata_managed_fixture') -or $metadataIntegrationInitializer.Contains('McwManagedTestHost.Initialize()')) { throw 'Integration suite already changed; review before preparing a patch.' }
$metadataIntegrationProject = $metadataIntegrationProject.Replace('</Project>', "`t<ItemGroup>`n`t`t<Compile Include=`"..\mcw\tests\psbt_metadata_managed_fixture.cs`" Link=`"Infrastructure\McwManagedTestHost.cs`" />`n`t</ItemGroup>`n`n</Project>")
$metadataIntegrationProject = $metadataIntegrationProject.Replace('<CopyLocalLockFileAssemblies>true</CopyLocalLockFileAssemblies>', '<CopyLocalLockFileAssemblies>true</CopyLocalLockFileAssemblies>' + "`n`t`t<AllowUnsafeBlocks>true</AllowUnsafeBlocks>")
$metadataIntegrationInitializer = $metadataIntegrationInitializer.Replace("internal static void Initialize()`n`t{", "internal static void Initialize()`n`t{`n`t`tglobal::MagicalCryptoWallet.Tests.Infrastructure.McwManagedTestHost.Initialize();`n")
if (-not $metadataIntegrationInitializer.Contains('McwManagedTestHost.Initialize();')) { throw 'Integration startup shape changed; review the hook.' }
[IO.File]::WriteAllText((Join-Path $metadataPatchRoot 'MagicalCryptoWallet.IntegrationTests.csproj'), $metadataIntegrationProject, [Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText((Join-Path $metadataPatchRoot 'IntegrationModuleInitializer.cs'), $metadataIntegrationInitializer, [Text.UTF8Encoding]::new($false))
$metadataPatch = [Collections.Generic.List[string]]::new()
foreach ($metadataFile in @('MagicalCryptoWallet.Tests.csproj','ModuleInitializer.cs','ManagedApplicationHost.cs','MagicalCryptoWallet.IntegrationTests.csproj','IntegrationModuleInitializer.cs')) {
    $metadataPath = switch ($metadataFile) {
        'ManagedApplicationHost.cs' { 'MagicalCryptoWallet.Client/Application/ManagedApplicationHost.cs' }
        'MagicalCryptoWallet.IntegrationTests.csproj' { 'MagicalCryptoWallet.IntegrationTests/MagicalCryptoWallet.IntegrationTests.csproj' }
        'IntegrationModuleInitializer.cs' { 'MagicalCryptoWallet.IntegrationTests/ModuleInitializer.cs' }
        default { "MagicalCryptoWallet.Tests/$metadataFile" }
    }
    $metadataLines = & git -C $metadataRoot diff --no-index -- (Join-Path $metadataSource $metadataPath) (Join-Path $metadataPatchRoot $metadataFile)
    if ($LASTEXITCODE -ne 1) { throw 'Expected exactly a prepared suite diff.' }
    foreach ($metadataLine in $metadataLines) {
        if ($metadataLine.StartsWith('diff --git ')) { $metadataPatch.Add("diff --git a/$metadataPath b/$metadataPath") }
        elseif ($metadataLine.StartsWith('--- ')) { $metadataPatch.Add("--- a/$metadataPath") }
        elseif ($metadataLine.StartsWith('+++ ')) { $metadataPatch.Add("+++ b/$metadataPath") }
        else { $metadataPatch.Add($metadataLine) }
    }
}
$metadataPatchPath = Join-Path $metadataRoot 'Contrib/McwMigration/Handoffs/psbt-metadata-tests.patch'
[IO.File]::WriteAllText($metadataPatchPath, ($metadataPatch -join "`n") + "`n", [Text.UTF8Encoding]::new($false))
Push-Location $metadataSource
try { & git apply --check --ignore-space-change $metadataPatchPath } finally { Pop-Location }
if ($LASTEXITCODE -ne 0) { throw 'Prepared suite patch does not apply.' }
$metadataCi = [IO.File]::ReadAllText((Join-Path $metadataSource '.github/workflows/build.yml')).Replace("`r`n", "`n")
$metadataSnapshotCi = [IO.File]::ReadAllText((Join-Path $metadataSource '.github/workflows/verify-software-snapshot.yml')).Replace("`r`n", "`n")
$metadataNativePackage = '.artifacts/packages/${{ matrix.rid }}/MagicalCryptoWallet/mcw${{ matrix.rid == ''win-x64'' && ''.exe'' || '''' }}'
$metadataCi = $metadataCi.Replace("dotnet test --project MagicalCryptoWallet.Tests -c Release --filter-namespace '*UnitTests*' --no-progress --no-ansi --output Normal", "pwsh -NoProfile -File mcw/tests/psbt_metadata_suite_verify.ps1 -NativeApplication $metadataNativePackage -FilterNamespace '*UnitTests*' -WaitForSlotSeconds 120")
$metadataCi = $metadataCi.Replace('dotnet test --project MagicalCryptoWallet.IntegrationTests -c Release --no-progress --no-ansi --output Normal', "pwsh -NoProfile -File mcw/tests/psbt_metadata_suite_verify.ps1 -NativeApplication $metadataNativePackage -TestProject MagicalCryptoWallet.IntegrationTests -AllTests -WaitForSlotSeconds 120")
$metadataCi = $metadataCi.Replace("dotnet test --project MagicalCryptoWallet.IntegrationTests -c Release --filter-class '*DesktopLifecycleTests' --no-progress --no-ansi --output Normal", "pwsh -NoProfile -File mcw/tests/psbt_metadata_suite_verify.ps1 -NativeApplication $metadataNativePackage -TestProject MagicalCryptoWallet.IntegrationTests -FilterClass '*DesktopLifecycleTests' -WaitForSlotSeconds 120")
$metadataSnapshotCi = $metadataSnapshotCi.Replace("dotnet test --project MagicalCryptoWallet.IntegrationTests -c Release --filter-class '*AutomaticCoinJoinTests' --no-progress --no-ansi --output Normal", "pwsh -NoProfile -File mcw/tests/psbt_metadata_suite_verify.ps1 -NativeApplication .artifacts/packages/linux-x64/MagicalCryptoWallet/mcw -TestProject MagicalCryptoWallet.IntegrationTests -FilterClass '*AutomaticCoinJoinTests' -WaitForSlotSeconds 120")
if ([regex]::Matches($metadataCi, 'psbt_metadata_suite_verify').Count -ne 3 -or [regex]::Matches($metadataSnapshotCi, 'psbt_metadata_suite_verify').Count -ne 1) { throw 'CI test invocation shape changed; review before preparing the atomic runner delta.' }
[IO.File]::WriteAllText((Join-Path $metadataPatchRoot 'build.yml'), $metadataCi, [Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText((Join-Path $metadataPatchRoot 'verify-software-snapshot.yml'), $metadataSnapshotCi, [Text.UTF8Encoding]::new($false))
$metadataCiPatch = [Collections.Generic.List[string]]::new()
foreach ($metadataFile in @('build.yml','verify-software-snapshot.yml')) {
    $metadataPath = ".github/workflows/$metadataFile"
    $metadataLines = & git -C $metadataRoot diff --no-index -- (Join-Path $metadataSource $metadataPath) (Join-Path $metadataPatchRoot $metadataFile)
    if ($LASTEXITCODE -ne 1) { throw 'Expected exactly a prepared CI runner diff.' }
    foreach ($metadataLine in $metadataLines) {
        if ($metadataLine.StartsWith('diff --git ')) { $metadataCiPatch.Add("diff --git a/$metadataPath b/$metadataPath") }
        elseif ($metadataLine.StartsWith('--- ')) { $metadataCiPatch.Add("--- a/$metadataPath") }
        elseif ($metadataLine.StartsWith('+++ ')) { $metadataCiPatch.Add("+++ b/$metadataPath") }
        else { $metadataCiPatch.Add($metadataLine) }
    }
}
$metadataCiPatchPath = Join-Path $metadataRoot 'Contrib/McwMigration/Handoffs/psbt-metadata-ci.patch'
[IO.File]::WriteAllText($metadataCiPatchPath, ($metadataCiPatch -join "`n") + "`n", [Text.UTF8Encoding]::new($false))
Push-Location $metadataSource
try { & git apply --check --ignore-space-change $metadataCiPatchPath } finally { Pop-Location }
if ($LASTEXITCODE -ne 0) { throw 'Prepared CI patch does not apply.' }
$metadataCombined = foreach ($metadataPart in @('psbt-metadata-host.patch','psbt-metadata-caller.patch','psbt-metadata-tests.patch','psbt-metadata-ci.patch')) {
    $metadataPartText = [IO.File]::ReadAllText((Join-Path $metadataRoot "Contrib/McwMigration/Handoffs/$metadataPart"))
    if (-not $metadataPartText.EndsWith("`n")) { $metadataPartText += "`n" }
    $metadataPartText
}
$metadataActivation = Join-Path $metadataRoot 'Contrib/McwMigration/Handoffs/psbt-metadata-activation.patch'
[IO.File]::WriteAllText($metadataActivation, ($metadataCombined -join ''), [Text.UTF8Encoding]::new($false))
Push-Location $metadataSource
try { & git apply --check --ignore-space-change $metadataActivation } finally { Pop-Location }
if ($LASTEXITCODE -ne 0) { throw 'Atomic host/caller/suite patch does not apply.' }
Write-Output "SUITE_PATCH=$metadataPatchPath"
