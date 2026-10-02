param(
    [string]$SharedRoot='C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',
    [string]$RustBin=''
)
$ErrorActionPreference='Stop'
$scopeRoot=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if (-not $RustBin) { $RustBin=Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin' }
$rustc=Join-Path $RustBin 'rustc.exe'
$rustfmt=Join-Path $RustBin 'rustfmt.exe'
$clippy=Join-Path $RustBin 'clippy-driver.exe'
$version=(& $rustc --version) -join ''
if ($LASTEXITCODE -or $version -notmatch '^rustc 1\.99\.0 ') { throw 'Rust 1.99.0 required.' }
$freeGiB=[double](Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory/1MB
if ($freeGiB -lt 2) { throw 'Deferred: fewer than2GiB free.' }
$slotHandle=$null
foreach ($slot in 1..2) {
    try { $slotHandle=[IO.File]::Open((Join-Path $SharedRoot ".artifacts/mcw-coordination/build-slot-$slot.lock"),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None); break } catch [IO.IOException] { }
}
if (-not $slotHandle) { throw 'Deferred: both build slots occupied.' }
try {
    if (-not $env:VCToolsInstallDir) {
        $vswhere=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
        $vsRoot=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if (-not $vsRoot) { throw 'Installed native linker unavailable.' }
        & (Join-Path $vsRoot 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
    }
    $output=Join-Path $scopeRoot '.artifacts/privacy-verification'
    [IO.Directory]::CreateDirectory($output) | Out-Null
    $sources=@(Get-ChildItem -LiteralPath (Join-Path $scopeRoot 'mcw/src/privacy_service') -Filter '*.rs' -Recurse -File | ForEach-Object FullName)
    $test=Join-Path $PSScriptRoot 'privacy_conformance.rs'
    & $rustfmt --edition 2024 --check @sources $test
    if ($LASTEXITCODE) { throw 'Formatting failed.' }
    $harness=Join-Path $output 'lib.rs'
    $encoding=(Join-Path $scopeRoot 'mcw/src/bitcoin_encoding.rs').Replace('\','/')
    $hashes=(Join-Path $scopeRoot 'mcw/src/wallet_hashes.rs').Replace('\','/')
    $privacy=(Join-Path $scopeRoot 'mcw/src/privacy_service/mod.rs').Replace('\','/')
    [IO.File]::WriteAllText($harness, "#[path=`"$encoding`"] pub mod bitcoin_encoding;`n#[path=`"$hashes`"] pub mod wallet_hashes;`n#[path=`"$privacy`"] pub mod privacy_service;`n")
    & $rustc --edition=2024 --crate-type=lib --emit=metadata -Dwarnings $harness -o (Join-Path $output 'privacy.rmeta')
    if ($LASTEXITCODE) { throw 'Module warnings failed.' }
    & $clippy --edition=2024 --crate-type=lib --emit=metadata -Dwarnings -Dclippy::all $harness -o (Join-Path $output 'privacy-clippy.rmeta')
    if ($LASTEXITCODE) { throw 'Module Clippy failed.' }
    $exe=Join-Path $output 'privacy_conformance.exe'
    & $rustc --edition=2024 --test -Dwarnings $test -o $exe
    if ($LASTEXITCODE) { throw 'Test compilation failed.' }
    & $exe --test-threads=1 2>&1 | Tee-Object -FilePath (Join-Path $output 'results.txt')
    if ($LASTEXITCODE) { throw 'Conformance failed.' }
    & dumpbin /dependents $exe | Set-Content -LiteralPath (Join-Path $output 'native-imports.txt')
    [ordered]@{ scope='privacy protocol checkpoint; not production Tor'; rust=$version; target='x86_64-pc-windows-msvc'; free_memory_gib=$freeGiB; verified_utc=[DateTime]::UtcNow.ToString('O'); production_integrated=$false; old_implementation_retired=$false; dependency_removed=$false; files=@($sources | ForEach-Object { [ordered]@{path=$_.Substring($scopeRoot.Length+1).Replace('\','/'); sha256=(Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash.ToLowerInvariant()} }) } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $output 'verification.json') -Encoding utf8
    Write-Output "Evidence saved: $output"
} finally { $slotHandle.Dispose() }
