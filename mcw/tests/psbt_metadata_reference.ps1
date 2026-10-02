param([string]$NBitcoinAssembly)
$ErrorActionPreference = 'Stop'
if (-not $NBitcoinAssembly) { $NBitcoinAssembly = Join-Path $env:USERPROFILE '.nuget/packages/nbitcoin/10.0.13/lib/net10.0/NBitcoin.dll' }
if (-not (Test-Path -LiteralPath $NBitcoinAssembly -PathType Leaf)) { throw 'Existing retained reference assembly is required.' }
Add-Type -Path $NBitcoinAssembly
$metadataReferences = @((Get-ChildItem (Join-Path $PSHOME 'ref') -Filter '*.dll').FullName) + @($NBitcoinAssembly)
Add-Type -Path (Join-Path $PSScriptRoot 'psbt_metadata_reference.cs') -ReferencedAssemblies $metadataReferences
$metadataRows = [PsbtMetadataReference]::Generate()
[IO.File]::WriteAllLines((Join-Path $PSScriptRoot 'psbt_metadata_vectors.tsv'), @($metadataRows | Where-Object { -not $_.StartsWith("large-parent`t") }), [Text.UTF8Encoding]::new($false))
$metadataReferenceOutput = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../.artifacts/psbt-metadata-reference'))
New-Item -ItemType Directory -Path $metadataReferenceOutput -Force | Out-Null
[IO.File]::WriteAllLines((Join-Path $metadataReferenceOutput 'generated-vectors.tsv'), $metadataRows, [Text.UTF8Encoding]::new($false))
Write-Output ('REFERENCE_SHA256=' + (Get-FileHash -LiteralPath $NBitcoinAssembly -Algorithm SHA256).Hash.ToLowerInvariant())
Write-Output ('REFERENCE_CASES=' + ($metadataRows.Count - 2))
