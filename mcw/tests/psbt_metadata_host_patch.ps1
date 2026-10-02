# Prepare a reviewable shared-owner patch without editing shared application
# roots or the active host checkout. Stop if the published host shape changed.
param([string]$SourceRoot)
$ErrorActionPreference = 'Stop'
$metadataRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$metadataSource = if ($SourceRoot) { [IO.Path]::GetFullPath($SourceRoot) } else { $metadataRoot }
$metadataPatchRoot = Join-Path $metadataRoot '.artifacts/psbt-metadata-host-patch'
New-Item -ItemType Directory -Path $metadataPatchRoot -Force | Out-Null
$metadataApp = [IO.File]::ReadAllText((Join-Path $metadataSource 'mcw/src/app.rs')).Replace("`r`n", "`n")
if ([regex]::Matches($metadataApp, '(?m)^[ \t]*bootstrap,$').Count -ne 2 -or $metadataApp.Contains('psbt_metadata')) { throw 'Host dispatch changed; review before preparing a patch.' }
$metadataApp = $metadataApp.Replace('let handshake_deadline = Instant::now() + Duration::from_secs(15);', 'let handshake_deadline = Instant::now() + Duration::from_secs(15);' + "`n    let mut psbt_metadata = crate::psbt_metadata_service::Transfers::default();")
$metadataApp = [regex]::Replace($metadataApp, '(?m)^([ \t]*)bootstrap,$', '$1bootstrap,' + "`n" + '$1&mut psbt_metadata,')
$metadataApp = $metadataApp.Replace("    bootstrap: &[u8],`n) -> io::Result<()> {", "    bootstrap: &[u8],`n    psbt_metadata: &mut crate::psbt_metadata_service::Transfers,`n) -> io::Result<()> {")
$metadataApp = $metadataApp.Replace("    {`n        return Ok(());`n    }`n    if frame.kind != bridge::REQUEST", "    {`n        psbt_metadata.cancel(frame.id);`n        return Ok(());`n    }`n    if frame.kind != bridge::REQUEST")
$metadataApp = $metadataApp.Replace('Ok(Ok(None)) | Err(mpsc::RecvTimeoutError::Disconnected) => {', 'Ok(Ok(None)) | Err(mpsc::RecvTimeoutError::Disconnected) => {' + "`n                psbt_metadata.clear();")
$metadataApp = $metadataApp.Replace('Ok(Err(error)) => {', 'Ok(Err(error)) => {' + "`n                psbt_metadata.clear();")
$metadataApp = $metadataApp.Replace('Event::Closed(failure) => {', 'Event::Closed(failure) => {' + "`n                psbt_metadata.clear();")
$metadataApp = $metadataApp.Replace("                ) {`n                    eprintln!", "                ) {`n                    psbt_metadata.clear();`n                    eprintln!")
$metadataRouting = @'
        crate::psbt_metadata_service::ENRICH..=crate::psbt_metadata_service::ABORT => {
            match psbt_metadata.handle(frame.id, frame.operation, &frame.payload) {
                Ok(payload) => frame.reply(payload).write(output),
                Err(error) => frame.error(error.code(), error.message()).write(output),
            }
        }
'@
$metadataApp = $metadataApp.Replace('        bridge::QR => bridge::encode_qr(frame).write(output),', '        bridge::QR => bridge::encode_qr(frame).write(output),' + "`n" + $metadataRouting.TrimEnd())
if ([regex]::Matches($metadataApp, '&mut psbt_metadata,').Count -ne 2 -or -not $metadataApp.Contains('psbt_metadata.cancel(frame.id)')) { throw 'Incomplete PSBT host routing patch.' }
[IO.File]::WriteAllText((Join-Path $metadataPatchRoot 'app.rs'), $metadataApp, [Text.UTF8Encoding]::new($false))
$metadataLib = [IO.File]::ReadAllText((Join-Path $metadataSource 'mcw/src/lib.rs')).Replace("`r`n", "`n")
if (-not $metadataLib.Contains('pub mod psbt_metadata;')) { $metadataLib = $metadataLib.Replace('pub mod psbt;', "pub mod psbt;`npub mod psbt_metadata;") }
if (-not $metadataLib.Contains('pub mod psbt_metadata_service;')) { $metadataLib = $metadataLib.Replace('pub mod psbt_metadata;', "pub mod psbt_metadata;`npub mod psbt_metadata_service;") }
[IO.File]::WriteAllText((Join-Path $metadataPatchRoot 'lib.rs'), $metadataLib, [Text.UTF8Encoding]::new($false))
$metadataPatch = [Collections.Generic.List[string]]::new()
foreach ($metadataFile in @('app.rs','lib.rs')) {
    $metadataLines = & git -C $metadataRoot diff --no-index -- (Join-Path $metadataSource "mcw/src/$metadataFile") (Join-Path $metadataPatchRoot $metadataFile)
    if ($LASTEXITCODE -eq 0) { continue }
    if ($LASTEXITCODE -ne 1) { throw 'Expected exactly a prepared host diff.' }
    foreach ($metadataLine in $metadataLines) {
        if ($metadataLine.StartsWith('diff --git ')) { $metadataPatch.Add("diff --git a/mcw/src/$metadataFile b/mcw/src/$metadataFile") }
        elseif ($metadataLine.StartsWith('--- ')) { $metadataPatch.Add("--- a/mcw/src/$metadataFile") }
        elseif ($metadataLine.StartsWith('+++ ')) { $metadataPatch.Add("+++ b/mcw/src/$metadataFile") }
        else { $metadataPatch.Add($metadataLine) }
    }
}
$metadataPatchPath = Join-Path $metadataRoot 'Contrib/McwMigration/Handoffs/psbt-metadata-host.patch'
[IO.File]::WriteAllText($metadataPatchPath, ($metadataPatch -join "`n") + "`n", [Text.UTF8Encoding]::new($false))
# Check at the selected source path; it may be an exact published-source archive.
Push-Location $metadataSource
try { & git apply --check --ignore-space-change $metadataPatchPath } finally { Pop-Location }
if ($LASTEXITCODE -ne 0) { throw 'Prepared host patch does not apply.' }
Write-Output "HOST_PATCH=$metadataPatchPath"
