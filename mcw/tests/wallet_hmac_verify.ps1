param([string]$SharedRoot='C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet', [IO.FileStream]$ExistingBuildSlot)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$evidence=Join-Path $root '.artifacts/wallet-hmac-evidence'
New-Item -ItemType Directory -Force -Path $evidence | Out-Null
$tools=Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin'
function FileHash([string]$path) { (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
$fixture=Join-Path $PSScriptRoot 'wallet_hmac_fixtures/independent.tsv'
$manifest=Get-Content -LiteralPath (Join-Path $PSScriptRoot 'wallet_hmac_fixtures/manifest.json') -Raw | ConvertFrom-Json
if ((FileHash $fixture) -ne $manifest.fixture_sha256) { throw 'Independent fixture checksum mismatch' }
$baseline=Join-Path $root 'mcw/src/wallet_hashes.rs'
$canonical=[Text.Encoding]::UTF8.GetBytes([IO.File]::ReadAllText($baseline).Replace("`r`n","`n"))
$canonicalHash=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($canonical)).ToLowerInvariant()
if ($canonicalHash -ne '752aec6cb51d2603f9c447f12e4c043425bb31129c5b2a473bd63b78eeeeb4a2') { throw 'Published hash checkpoint source changed; stop incorporation' }
if ((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory -lt 2097152) { Write-Output 'BUILD_DEFERRED'; exit 3 }
$handle=$ExistingBuildSlot
$ownsHandle=$null -eq $ExistingBuildSlot
if ($ExistingBuildSlot) {
 $validNames=@(1,2 | ForEach-Object {[IO.Path]::GetFullPath((Join-Path $SharedRoot ".artifacts/mcw-coordination/build-slot-$_.lock"))})
 if (-not $ExistingBuildSlot.CanRead -or -not $ExistingBuildSlot.CanWrite -or $ExistingBuildSlot.Name -notin $validNames) { throw 'Existing build slot is invalid' }
} else {
 foreach ($slot in 1,2) {
  try { $handle=[IO.File]::Open((Join-Path $SharedRoot ".artifacts/mcw-coordination/build-slot-$slot.lock"),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None); break } catch [IO.IOException] {}
 }
}
if (-not $handle) { Write-Output 'BUILD_SLOTS_BUSY'; exit 3 }
$oldPath=$env:PATH; $oldLib=$env:LIB; $oldJobs=$env:CARGO_BUILD_JOBS
try {
 $compiler=& (Join-Path $tools rustc.exe) --version
 if ($LASTEXITCODE -ne 0 -or $compiler -notlike 'rustc 1.99.0 *') { throw 'Rust 1.99 required' }
 $sources=@('mcw/src/bitcoin_encoding.rs','mcw/src/wallet_hashes.rs','mcw/src/wallet_hash_service.rs')
 $harness="extern crate self as mcw;`n"
 foreach ($pair in @(@('bitcoin_encoding',$sources[0]),@('wallet_hashes',$sources[1]),@('hash_tests','mcw/tests/wallet_hashes_conformance.rs'),@('service_tests','mcw/tests/wallet_hmac_conformance.rs'))) {
  $harness+='#[path="'+(Join-Path $root $pair[1]).Replace('\','/')+'"] pub mod '+$pair[0]+";`n"
 }
 $harnessPath=Join-Path $evidence 'actual-source-tests.rs'; [IO.File]::WriteAllText($harnessPath,$harness)
 & (Join-Path $tools rustfmt.exe) --edition 2024 --check (Join-Path $root $sources[2]) (Join-Path $PSScriptRoot 'wallet_hmac_conformance.rs')
 if ($LASTEXITCODE -ne 0) { throw 'Formatting check failed' }
 & (Join-Path $tools clippy-driver.exe) --edition=2024 --test --emit=metadata -D warnings $harnessPath -o (Join-Path $evidence 'tests.rmeta')
 if ($LASTEXITCODE -ne 0) { throw 'Clippy failed' }
 $linker=Get-ChildItem 'C:\Program Files\Microsoft Visual Studio\*\*\VC\Tools\MSVC\*\bin\Hostx64\x64\link.exe' | Sort-Object FullName -Descending | Select-Object -First 1
 $kernel=Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\Lib\*\um\x64\kernel32.lib' | Sort-Object FullName -Descending | Select-Object -First 1
 if (-not $linker -or -not $kernel) { throw 'Existing test linker/SDK unavailable' }
 $msvc=[IO.Path]::GetFullPath((Join-Path $linker.Directory.FullName '../../..')); $sdk=[IO.Path]::GetFullPath((Join-Path $kernel.Directory.FullName '../..'))
 $env:PATH=$linker.Directory.FullName+';'+$env:PATH
 $env:LIB=(Join-Path $msvc 'lib/onecore/x64')+';'+(Join-Path $sdk 'ucrt/x64')+';'+(Join-Path $sdk 'um/x64'); $env:CARGO_BUILD_JOBS='1'
 $runs=@()
 foreach ($profile in 'debug','optimized') {
  $binary=Join-Path $evidence "service-$profile.exe"; $log=Join-Path $evidence "$profile-tests.txt"
  $argsList=@('--edition=2024','--test','-D','warnings','-C','overflow-checks=yes','-C','codegen-units=1','-C','target-feature=+crt-static',$harnessPath,'-o',$binary)
  if ($profile -eq 'optimized') { $argsList+=@('-C','opt-level=3') }
  & (Join-Path $tools rustc.exe) @argsList
  if ($LASTEXITCODE -ne 0) { throw 'Actual-source compilation failed' }
  & $binary --test-threads=1 2>&1 | Tee-Object -FilePath $log
  if ($LASTEXITCODE -ne 0) { throw "$profile actual-source tests failed" }
  $runs+=@{profile=$profile;log=$log;binary_sha256=FileHash $binary}
 }
 $hashes=[ordered]@{}
 foreach ($path in $sources+@('mcw/tests/wallet_hmac_conformance.rs')) { $hashes[$path]=FileHash (Join-Path $root $path) }
 $result=@{compiler=$compiler;verified_at_utc=[DateTime]::UtcNow.ToString('o');source_hashes=$hashes;published_hash_checkpoint_sha256_lf=$canonicalHash;runs=$runs;independent_cases=$manifest.cases;primary_hash_vectors=569;production_caller_incorporation=$false;note='Ignored actual-source executables only; host/managed caller incorporation verified separately'}
 $result | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $evidence 'verification.json') -Encoding utf8NoBOM
 $result | ConvertTo-Json -Depth 8 -Compress | Write-Output
} finally { $env:PATH=$oldPath; $env:LIB=$oldLib; $env:CARGO_BUILD_JOBS=$oldJobs; if ($ownsHandle) { $handle.Dispose() } }
