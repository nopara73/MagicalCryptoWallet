param([Parameter(Mandatory)][string]$ActivationSourceRoot, [ValidateRange(0,300)][int]$WaitForSlotSeconds = 0)
$ErrorActionPreference = 'Stop'
if (-not [OperatingSystem]::IsWindows()) { throw 'This verifier records the Windows shipping-runtime policy only.' }
$metadataRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$metadataSource = [IO.Path]::GetFullPath($ActivationSourceRoot)
$metadataFactory = Join-Path $metadataSource 'MagicalCryptoWallet/Blockchain/Transactions/TransactionFactory.cs'
if (-not [IO.File]::ReadAllText($metadataFactory).Contains('psbt = MagicalCryptoWallet.Mcw.Psbt.McwPsbtMetadata.Enrich(')) { throw 'An isolated, atomically activated source snapshot is required.' }
if (-not [IO.File]::ReadAllText((Join-Path $metadataSource 'mcw/src/app.rs')).Contains('psbt_metadata.handle(')) { throw 'The snapshot must include real host metadata dispatch.' }
$metadataCommon = & git -C $metadataRoot rev-parse --path-format=absolute --git-common-dir
if ($LASTEXITCODE -ne 0) { throw 'Cannot locate shared build coordination root.' }
$metadataShared = Split-Path -Parent ($metadataCommon | Select-Object -First 1)
$metadataRust = Join-Path $metadataShared '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin'
$metadataRun = (Get-Date -Format 'yyyyMMdd-HHmmss-fff') + '-' + [guid]::NewGuid().ToString('N').Substring(0,8)
$metadataOutput = Join-Path $metadataRoot ".artifacts/psbt-shipping-proof/$metadataRun"
New-Item -ItemType Directory -Path $metadataOutput -Force | Out-Null
function Get-MetadataSourceHashes {
    $metadataFiles = [Collections.Generic.List[string]]::new()
    foreach ($metadataDirectory in @('mcw/src','MagicalCryptoWallet','MagicalCryptoWallet.Client','MagicalCryptoWallet.Tests','MagicalCryptoWallet.IntegrationTests')) {
        foreach ($metadataFile in [IO.Directory]::GetFiles((Join-Path $metadataSource $metadataDirectory), '*', [IO.SearchOption]::AllDirectories)) {
            $metadataRelative = [IO.Path]::GetRelativePath($metadataSource, $metadataFile).Replace('\','/')
            if ($metadataRelative -notmatch '/(?:bin|obj)/' -and $metadataRelative -match '\.(?:rs|cs|csproj|props|targets)$') { $metadataFiles.Add($metadataRelative) }
        }
    }
    $metadataFiles.AddRange([string[]]@('mcw/Cargo.toml','mcw/Cargo.lock','mcw/build.rs','Contrib/Mcw/build-windows.ps1','Contrib/Mcw/audit.py','Directory.Build.props','Directory.Build.targets','Directory.Packages.props','BannedSymbols.txt','global.json','mcw/tests/psbt_metadata_managed_fixture.cs'))
    $metadataHashes = [ordered]@{}
    foreach ($metadataFile in ($metadataFiles | Sort-Object -Unique)) { $metadataHashes[$metadataFile] = (Get-FileHash -LiteralPath (Join-Path $metadataSource $metadataFile)).Hash.ToLowerInvariant() }
    return $metadataHashes
}
$metadataBefore = Get-MetadataSourceHashes
$metadataBefore | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $metadataOutput 'source-before.json') -Encoding utf8
$metadataDevelopment = Join-Path $metadataSource 'native-build/debug/mcw.exe'
$metadataDevelopmentBefore = if (Test-Path -LiteralPath $metadataDevelopment -PathType Leaf) { (Get-FileHash -LiteralPath $metadataDevelopment).Hash.ToLowerInvariant() } else { $null }
$metadataSlot = $null
$metadataDeadline = [DateTime]::UtcNow.AddSeconds($WaitForSlotSeconds)
do {
    foreach ($metadataIndex in 1..2) {
        try { $metadataSlot = [IO.File]::Open((Join-Path $metadataShared ".artifacts/mcw-coordination/build-slot-$metadataIndex.lock"), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None); break } catch [IO.IOException] { }
    }
    if ($metadataSlot -or [DateTime]::UtcNow -ge $metadataDeadline) { break }
    Start-Sleep -Seconds 2
} while ($true)
if (-not $metadataSlot) { throw 'Both build slots occupied; preserve source fingerprints and retry later.' }
try {
    Write-Output "BUILD_SLOT=$metadataIndex VERIFIER_PID=$PID ASSIGNMENT=psbt_metadata_shipping_runtime"
    if ((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory * 1KB -lt 2GB) { throw 'Less than 2 GiB free RAM; defer compilation.' }
    & 'C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/Common7/Tools/Launch-VsDevShell.ps1' -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
    $env:PATH = $metadataRust + ';' + $env:PATH
    $env:RUSTUP_HOME = Join-Path $metadataShared '.artifacts/mcw-tools/rustup'
    $env:CARGO_HOME = Join-Path $metadataShared '.artifacts/mcw-tools/cargo'
    $env:RUSTC = Join-Path $metadataRust 'rustc.exe'
    $env:CARGO_BUILD_JOBS = '1'
    $env:CARGO_PROFILE_RELEASE_CODEGEN_UNITS = '1'
    $env:CARGO_TARGET_DIR = Join-Path $metadataOutput 'native-target'
    $metadataCargo = Join-Path $metadataRust 'cargo.exe'
    # Use the published shipping recipe unchanged, including the actual entry/TLS
    # runtime, matching std+panic_abort, and strict offline application graph.
    & (Join-Path $metadataSource 'Contrib/Mcw/build-windows.ps1') -Cargo $metadataCargo 2>&1 | Tee-Object -FilePath (Join-Path $metadataOutput 'shipping-build.log')
    if ($LASTEXITCODE -ne 0) { throw 'Published shipping-runtime recipe failed.' }
    $metadataBinary = Join-Path $env:CARGO_TARGET_DIR 'x86_64-pc-windows-msvc/release/mcw.exe'
    & python (Join-Path $metadataSource 'Contrib/Mcw/audit.py') --binary $metadataBinary 2>&1 | Tee-Object -FilePath (Join-Path $metadataOutput 'strict-import-audit.log')
    if ($LASTEXITCODE -ne 0) { throw 'Actual direct/delay OS import allowlist audit failed.' }
    Copy-Item -LiteralPath (Join-Path $metadataSource '.artifacts/mcw-evidence/mcw.exe.imports.json') -Destination (Join-Path $metadataOutput 'imports.json')
    & dumpbin.exe /nologo /imports $metadataBinary | Set-Content -LiteralPath (Join-Path $metadataOutput 'dumpbin-imports.txt') -Encoding utf8
    if ($LASTEXITCODE -ne 0) { throw 'Native PE import dump failed.' }
    $metadataImports = Get-Content -LiteralPath (Join-Path $metadataOutput 'imports.json') -Raw | ConvertFrom-Json
    $metadataBinaryHash = (Get-FileHash -LiteralPath $metadataBinary).Hash.ToLowerInvariant()
    if ($metadataImports.sha256 -ne $metadataBinaryHash -or [IO.Path]::GetFullPath($metadataImports.binary) -ne [IO.Path]::GetFullPath($metadataBinary)) { throw 'The import audit does not identify this exact binary.' }
    & $metadataCargo metadata --manifest-path (Join-Path $metadataSource 'mcw/Cargo.toml') --locked --offline --format-version 1 | Set-Content -LiteralPath (Join-Path $metadataOutput 'cargo-metadata.json') -Encoding utf8
    if ($LASTEXITCODE -ne 0) { throw 'Actual application Cargo graph audit failed.' }
    $metadataGraph = Get-Content -LiteralPath (Join-Path $metadataOutput 'cargo-metadata.json') -Raw | ConvertFrom-Json
    if ($metadataGraph.packages.Count -ne 1 -or $metadataGraph.packages[0].dependencies.Count) { throw 'Unexpected application dependency graph.' }
    $metadataAfter = Get-MetadataSourceHashes
    $metadataAfter | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $metadataOutput 'source-after.json') -Encoding utf8
    foreach ($metadataFile in $metadataBefore.Keys) { if ($metadataBefore[$metadataFile] -ne $metadataAfter[$metadataFile]) { throw "Build changed source: $metadataFile" } }
    if ($metadataBefore.Count -ne $metadataAfter.Count) { throw 'Build changed the source file set.' }
    $metadataDevelopmentAfter = if (Test-Path -LiteralPath $metadataDevelopment -PathType Leaf) { (Get-FileHash -LiteralPath $metadataDevelopment).Hash.ToLowerInvariant() } else { $null }
    if ($metadataDevelopmentBefore -ne $metadataDevelopmentAfter) { throw 'Historical development binary changed during the shipping-policy build.' }
    [ordered]@{activation_source_root=$metadataSource;native_binary=$metadataBinary;native_sha256=$metadataBinaryHash;runtime_imports=$metadataImports.runtime_imports;direct_and_delay_os_import_audit='passed';application_packages=1;external_cargo_dependencies=0;source_fingerprints_unchanged=$true;source_files=$metadataBefore.Count;policy_sha256=$metadataBefore['Contrib/Mcw/build-windows.ps1'];shipping_build_policy='published_windows_first_party_runtime_and_rebuilt_aborting_std';development_binary_preserved=($null -ne $metadataDevelopmentBefore);development_binary=$metadataDevelopment;development_sha256=$metadataDevelopmentBefore;production_verified=$false;nbitcoin_removed=$false;unix_verified=$false;five_target_verified=$false;queued_session_cleanup_verified=$false;wallet_transport_suite_verified=$false} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $metadataOutput 'evidence.json') -Encoding utf8
    Write-Output "SHIPPING_POLICY_BINARY=$metadataBinary"
    Write-Output "SHIPPING_POLICY_EVIDENCE=$metadataOutput"
} finally { $metadataSlot.Dispose() }
