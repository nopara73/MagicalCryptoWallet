param([string]$NBitcoinAssembly)

# Independent transitional-app reference only. Nothing here ships in mcw.
# Uses an already cached retained package; it performs no restore or install.
$ErrorActionPreference = 'Stop'
$psbtRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if (-not $NBitcoinAssembly) {
    $NBitcoinAssembly = Join-Path $env:USERPROFILE '.nuget/packages/nbitcoin/10.0.13/lib/net10.0/NBitcoin.dll'
}
if (-not (Test-Path -LiteralPath $NBitcoinAssembly -PathType Leaf)) { throw 'The retained NBitcoin reference assembly must already exist.' }
Add-Type -Path $NBitcoinAssembly
$psbtReferenceDirectory = Join-Path $psbtRoot '.artifacts/psbt-evidence'
New-Item -ItemType Directory -Path $psbtReferenceDirectory -Force | Out-Null
$psbtReference = [Collections.Generic.List[string]]::new()
$psbtExports = [Collections.Generic.List[string]]::new()
$psbtReference.Add('# All official cases tested with the already cached transitional NBitcoin assembly.')
$psbtExports.Add('# NBitcoin 10.0.13 canonical binary/Base64 exports from public BIP174/BIP370 vectors.')
$psbtExports.Add('# Source vectors: bitcoin/bips 3a10b5b5f0a7586df8928d580a3009744ebb2079; author Ava Chow; BSD-2-Clause (see psbt_vectors.md).')
$psbtAssemblyHash = (Get-FileHash -LiteralPath $NBitcoinAssembly -Algorithm SHA256).Hash.ToLowerInvariant()
$psbtReference.Add("# NBitcoin assembly SHA256 $psbtAssemblyHash")
$psbtExports.Add("# NBitcoin assembly SHA256 $psbtAssemblyHash")
$psbtAccepted = 0
$psbtRejected = 0
foreach ($psbtLine in Get-Content -LiteralPath (Join-Path $PSScriptRoot 'psbt_vectors.tsv')) {
    if ($psbtLine.StartsWith('#')) { continue }
    $psbtColumns = $psbtLine.Split("`t")
    $psbtBytes = [Convert]::FromHexString($psbtColumns[4])
    try {
        $psbtParsed = [NBitcoin.PSBT]::Load($psbtBytes, [NBitcoin.Network]::Main)
        $psbtSerialized = $psbtParsed.ToBytes()
        $psbtBase64 = $psbtParsed.ToBase64()
        if ($psbtBase64 -ne [Convert]::ToBase64String($psbtSerialized)) { throw 'Managed binary and Base64 exports differ.' }
        $psbtAccepted++
        $psbtReference.Add(($psbtColumns[0..3] + @('accepted', [Convert]::ToHexString($psbtSerialized).ToLowerInvariant(), $psbtBase64)) -join "`t")
        if ($psbtColumns[1] -ne 'invalid') {
            $psbtExports.Add(($psbtColumns[0..3] + @([Convert]::ToHexString($psbtSerialized).ToLowerInvariant(), $psbtBase64)) -join "`t")
        }
    } catch {
        $psbtRejected++
        $psbtReference.Add(($psbtColumns[0..3] + @('rejected', $_.Exception.GetType().FullName)) -join "`t")
    }
}
[IO.File]::WriteAllLines((Join-Path $psbtReferenceDirectory 'nbitcoin-reference-results.tsv'), $psbtReference, [Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllLines((Join-Path $PSScriptRoot 'psbt_managed_vectors.tsv'), $psbtExports, [Text.UTF8Encoding]::new($false))
$psbtSummary = [ordered]@{
    reference_assembly = [NBitcoin.PSBT].Assembly.FullName
    reference_sha256 = $psbtAssemblyHash
    fixture_sha256 = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'psbt_vectors.tsv') -Algorithm SHA256).Hash.ToLowerInvariant()
    accepted = $psbtAccepted
    rejected = $psbtRejected
    exported_container_valid_rows = $psbtExports.Count - 3
    shipping_dependency = $false
}
$psbtSummary | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $psbtReferenceDirectory 'nbitcoin-reference-summary.json') -Encoding utf8
$psbtSummary | ConvertTo-Json
