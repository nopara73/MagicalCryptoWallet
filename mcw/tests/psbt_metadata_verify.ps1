param([switch]$Optimized, [ValidateRange(0,300)][int]$WaitForSlotSeconds = 0)
$ErrorActionPreference = 'Stop'
$metadataRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$metadataCommon = & git -C $metadataRoot rev-parse --path-format=absolute --git-common-dir
if ($LASTEXITCODE -ne 0) { throw 'Cannot locate shared coordination root.' }
$metadataShared = Split-Path -Parent ($metadataCommon | Select-Object -First 1)
$metadataRust = Join-Path $metadataShared '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin'
$metadataRun = (Get-Date -Format 'yyyyMMdd-HHmmss-fff') + '-' + [guid]::NewGuid().ToString('N').Substring(0,8)
$metadataOutput = Join-Path $metadataRoot ".artifacts/psbt-metadata-validation/$metadataRun"
$metadataSnapshot = Join-Path $metadataOutput 'snapshot'
New-Item -ItemType Directory -Path $metadataSnapshot -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $metadataSnapshot 'src'), (Join-Path $metadataSnapshot 'tests') -Force | Out-Null
$metadataInputs = [ordered]@{}
foreach ($metadataModule in @('bitcoin_encoding','bitcoin_script','bitcoin_wire','wallet_hashes','psbt','psbt_metadata','psbt_metadata_service')) {
    $metadataInputs["$metadataModule.rs"] = Join-Path $metadataRoot "mcw/src/$metadataModule.rs"
}
$metadataInputs['psbt_metadata_conformance.rs'] = Join-Path $PSScriptRoot 'psbt_metadata_conformance.rs'
$metadataInputs['psbt_metadata_vectors.tsv'] = Join-Path $PSScriptRoot 'psbt_metadata_vectors.tsv'
$metadataHashes = foreach ($metadataEntry in $metadataInputs.GetEnumerator()) {
    $metadataDirectory = if ($metadataEntry.Key -in @('psbt_metadata_conformance.rs', 'psbt_metadata_vectors.tsv')) { 'tests' } else { 'src' }
    $metadataDestination = Join-Path (Join-Path $metadataSnapshot $metadataDirectory) $metadataEntry.Key
    Copy-Item -LiteralPath $metadataEntry.Value -Destination $metadataDestination
    [ordered]@{file=$metadataEntry.Key;source=$metadataEntry.Value;sha256=(Get-FileHash -LiteralPath $metadataDestination -Algorithm SHA256).Hash.ToLowerInvariant()}
}
$metadataHarness = 'extern crate self as mcw;' + [Environment]::NewLine
foreach ($metadataModule in @('bitcoin_encoding','bitcoin_script','bitcoin_wire','wallet_hashes','psbt','psbt_metadata','psbt_metadata_service')) {
    $metadataHarness += '#[path="snapshot/src/' + $metadataModule + '.rs"] pub mod ' + $metadataModule + ';' + [Environment]::NewLine
}
$metadataHarness += '#[path="snapshot/tests/psbt_metadata_conformance.rs"] pub mod conformance;'
$metadataHarnessPath = Join-Path $metadataOutput 'harness.rs'
[IO.File]::WriteAllText($metadataHarnessPath, $metadataHarness, [Text.UTF8Encoding]::new($false))
$metadataSlot = $null
$metadataSlotNumber = $null
$metadataSlotDeadline = [DateTime]::UtcNow.AddSeconds($WaitForSlotSeconds)
do {
    foreach ($metadataIndex in 1..2) {
        try {
            $metadataSlot = [IO.File]::Open((Join-Path $metadataShared ".artifacts/mcw-coordination/build-slot-$metadataIndex.lock"), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
            $metadataSlotNumber = $metadataIndex
            break
        } catch [IO.IOException] { }
    }
    if ($null -ne $metadataSlot -or [DateTime]::UtcNow -ge $metadataSlotDeadline) { break }
    Start-Sleep -Seconds 2
} while ($true)
if ($null -eq $metadataSlot) { throw 'Both build slots occupied; source snapshot preserved.' }
try {
    Write-Output "BUILD_SLOT=$metadataSlotNumber VERIFIER_PID=$PID ASSIGNMENT=psbt_metadata"
    if ((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory * 1KB -lt 2GB) { throw 'Less than 2 GiB free RAM; defer compilation.' }
    $metadataVersion = & (Join-Path $metadataRust 'rustc.exe') --version
    if ($LASTEXITCODE -ne 0 -or $metadataVersion -notmatch '^rustc 1\.99\.0 ') { throw 'Rust 1.99.0 is required.' }
    & 'C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/Common7/Tools/Launch-VsDevShell.ps1' -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
    & (Join-Path $metadataRust 'rustfmt.exe') --edition 2024 --check $metadataInputs['psbt_metadata.rs'] $metadataInputs['psbt_metadata_service.rs'] $metadataInputs['psbt_metadata_conformance.rs']
    if ($LASTEXITCODE -ne 0) { throw 'Metadata formatting failed.' }
    & (Join-Path $metadataRust 'clippy-driver.exe') --edition=2024 --test --emit=metadata -D warnings $metadataHarnessPath -o (Join-Path $metadataOutput 'clippy.rmeta') 2>&1 | Tee-Object -FilePath (Join-Path $metadataOutput 'clippy.log')
    if ($LASTEXITCODE -ne 0) { throw 'Metadata Clippy failed.' }
    $metadataBinary = Join-Path $metadataOutput 'metadata-conformance.exe'
    $metadataArguments = @('--edition=2024','--test','-C','codegen-units=1','-C','overflow-checks=yes','-D','warnings',$metadataHarnessPath,'-o',$metadataBinary)
    if ($Optimized) { $metadataArguments += '-O' }
    & (Join-Path $metadataRust 'rustc.exe') @metadataArguments 2>&1 | Tee-Object -FilePath (Join-Path $metadataOutput 'compile.log')
    if ($LASTEXITCODE -ne 0) { throw 'Metadata compilation failed.' }
    $env:MCW_PSBT_METADATA_REFERENCE = Join-Path $metadataRoot '.artifacts/psbt-metadata-reference/generated-vectors.tsv'
    if (-not (Test-Path -LiteralPath $env:MCW_PSBT_METADATA_REFERENCE)) { throw 'Run psbt_metadata_reference.ps1 first for the large-parent comparison.' }
    & $metadataBinary --test-threads=1 2>&1 | Tee-Object -FilePath (Join-Path $metadataOutput 'tests.log')
    if ($LASTEXITCODE -ne 0) { throw 'Metadata conformance failed.' }
    [ordered]@{ compiler=$metadataVersion;target='x86_64-pc-windows-msvc';optimized=[bool]$Optimized;format='passed';clippy='passed with warnings denied';tests='passed';build_slot=$metadataSlotNumber;inputs=@($metadataHashes);reference_sha256=(Get-FileHash -LiteralPath $env:MCW_PSBT_METADATA_REFERENCE).Hash.ToLowerInvariant();production_host_verified=$false } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $metadataOutput 'evidence.json') -Encoding utf8
    Write-Output "METADATA_EVIDENCE=$metadataOutput"
} finally { $metadataSlot.Dispose() }
