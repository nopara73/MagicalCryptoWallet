param(
 [string]$SharedRoot='C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',
 [string]$ManagedHostSource,
 [string]$RustSourceRoot,
 [string]$Python='C:\Python314\python.exe'
)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../..'))
if (-not $ManagedHostSource) { $ManagedHostSource=Join-Path $root 'MagicalCryptoWallet.Client/Application/ManagedApplicationHost.cs' }
if (-not $RustSourceRoot) { $RustSourceRoot=Join-Path $root 'mcw/src' }
$ManagedHostSource=[IO.Path]::GetFullPath($ManagedHostSource)
$RustSourceRoot=[IO.Path]::GetFullPath($RustSourceRoot)
$staged=$ManagedHostSource -ne (Join-Path $root 'MagicalCryptoWallet.Client/Application/ManagedApplicationHost.cs') -or $RustSourceRoot -ne (Join-Path $root 'mcw/src')
# Caller activation and shared dispatch must be supplied together in an isolated
# candidate. This verifier never changes a production source file or applies patches.
foreach ($relative in 'MagicalCryptoWallet/Crypto/OwnershipIdentifier.cs','MagicalCryptoWallet/Crypto/Slip21Node.cs') {
 $text=[IO.File]::ReadAllText((Join-Path $root $relative))
 if ($text -notmatch 'WalletHmac\.' -or $text -match 'HMACSHA(256|512)') { throw 'Apply the bounded caller patch in the verification candidate first' }
}
$hostText=[IO.File]::ReadAllText($ManagedHostSource)
$dispatchText=[IO.File]::ReadAllText((Join-Path $RustSourceRoot 'app.rs'))
if ($hostText -notmatch 'Invalid application error encoding' -or $dispatchText -notmatch 'wallet_hash_service::execute') { throw 'Apply the reviewed shared host patch in the verification candidate first' }
if ((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory -lt 2097152) { Write-Output 'BUILD_DEFERRED'; exit 3 }
$handle=$null
foreach ($slot in 1,2) {
 try { $handle=[IO.File]::Open((Join-Path $SharedRoot ".artifacts/mcw-coordination/build-slot-$slot.lock"),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None); break } catch [IO.IOException] {}
}
if (-not $handle) { Write-Output 'BUILD_SLOTS_BUSY'; exit 3 }
$oldPath=$env:PATH; $oldLib=$env:LIB
try {
 Write-Output ("BUILD_SLOT_ACQUIRED assignment=bounded-HMAC-caller-proof slot="+$slot+" verifier_pid="+$PID)
 $evidence=Join-Path $root '.artifacts/wallet-hmac-evidence/caller-probe'
 New-Item -ItemType Directory -Force -Path $evidence | Out-Null
 $tools=Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin'
 $compiler=& (Join-Path $tools rustc.exe) --version
 if ($LASTEXITCODE -ne 0 -or $compiler -notlike 'rustc 1.99.0 *') { throw 'Rust 1.99 required' }
 $canonical=[Text.Encoding]::UTF8.GetBytes([IO.File]::ReadAllText((Join-Path $RustSourceRoot 'wallet_hashes.rs')).Replace("`r`n","`n"))
 if ([Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($canonical)).ToLowerInvariant() -ne '752aec6cb51d2603f9c447f12e4c043425bb31129c5b2a473bd63b78eeeeb4a2') { throw 'Published hash checkpoint changed' }
 $projectDirectory=Join-Path $evidence 'project'
 New-Item -ItemType Directory -Force -Path $projectDirectory | Out-Null
 $project=Join-Path $projectDirectory 'HmacProbe.csproj'
 Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'HmacProbe.csproj.inc') -Destination $project
 Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'packages.lock.json.inc') -Destination (Join-Path $projectDirectory 'packages.lock.json')
 & dotnet build $project -c Release -m:1 -nr:false -p:UseSharedCompilation=false -p:BuildMcwHost=false -p:RestoreLockedMode=true "-p:HmacHostSource=$ManagedHostSource" "-p:HmacCoreProject=$(Join-Path $root 'MagicalCryptoWallet/MagicalCryptoWallet.csproj')" "-p:HmacProbeSourceRoot=$PSScriptRoot" 2>&1 | Tee-Object -FilePath (Join-Path $evidence 'managed-build.txt')
 if ($LASTEXITCODE -ne 0) { throw 'Actual managed caller build failed' }
 $run=Join-Path $evidence ('run-'+[Guid]::NewGuid().ToString('N'))
 New-Item -ItemType Directory -Path $run | Out-Null
 Copy-Item -Path (Join-Path $projectDirectory 'bin/Release/net10.0/*') -Destination $run -Recurse
 Copy-Item -LiteralPath (Join-Path $run 'HmacProbe.exe') -Destination (Join-Path $run 'magicalcryptowalletd.exe')
 $linker=Get-ChildItem 'C:\Program Files\Microsoft Visual Studio\*\*\VC\Tools\MSVC\*\bin\Hostx64\x64\link.exe' | Sort-Object FullName -Descending | Select-Object -First 1
 $kernel=Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\Lib\*\um\x64\kernel32.lib' | Sort-Object FullName -Descending | Select-Object -First 1
 if (-not $linker -or -not $kernel) { throw 'Existing test linker/SDK unavailable' }
 $msvc=[IO.Path]::GetFullPath((Join-Path $linker.Directory.FullName '../../..')); $sdk=[IO.Path]::GetFullPath((Join-Path $kernel.Directory.FullName '../..'))
 $env:PATH=$linker.Directory.FullName+';'+$env:PATH
 $env:LIB=(Join-Path $msvc 'lib/onecore/x64')+';'+(Join-Path $sdk 'ucrt/x64')+';'+(Join-Path $sdk 'um/x64')
 # Ignored tooling snapshot only. This CRT flag is never a production host build
 # or evidence of removing the Windows shipping runtime.
 $argsList=@('--edition=2024','--check-cfg=cfg(mcw_windows_runtime)','--check-cfg=cfg(test)','-C','overflow-checks=yes','-C','codegen-units=1','-C','target-feature=+crt-static','-C','opt-level=2')
 $lib=Join-Path $evidence 'libmcw.rlib'
 & (Join-Path $tools clippy-driver.exe) @argsList --crate-name mcw --crate-type=rlib --emit=metadata -D warnings (Join-Path $RustSourceRoot 'lib.rs') -o (Join-Path $evidence 'host.rmeta')
 if ($LASTEXITCODE -ne 0) { throw 'Actual Rust host Clippy failed' }
 & (Join-Path $tools rustc.exe) @argsList --crate-name mcw --crate-type=rlib (Join-Path $RustSourceRoot 'lib.rs') -o $lib
 if ($LASTEXITCODE -ne 0) { throw 'Actual Rust host library build failed' }
 & (Join-Path $tools rustc.exe) @argsList --extern "mcw=$lib" (Join-Path $RustSourceRoot 'main.rs') -o (Join-Path $run 'mcw.exe')
 if ($LASTEXITCODE -ne 0) { throw 'Actual Rust host executable build failed' }
 & $Python (Join-Path $PSScriptRoot 'verify.py') $root $RustSourceRoot $ManagedHostSource $run $evidence $staged.ToString().ToLowerInvariant()
 if ($LASTEXITCODE -ne 0) { throw 'Actual caller execution failed' }
} finally { $env:PATH=$oldPath; $env:LIB=$oldLib; $handle.Dispose() }
