param([string]$CoordinationRoot, [string]$RustBin, [string]$Python = 'python')

# Synthetic verification only. The coordinator reference and probe never become
# a fallback or a shipping Rust dependency. Hold one build slot, never Git lock.
$ErrorActionPreference = 'Stop'
$roundRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if (-not $CoordinationRoot) {
    $roundCommon = & git -C $roundRoot rev-parse --path-format=absolute --git-common-dir
    if ($LASTEXITCODE) { throw 'Cannot locate coordination root.' }
    $CoordinationRoot = Split-Path -Parent ($roundCommon | Select-Object -First 1)
}
if (-not $RustBin) {
    $RustBin = if ($IsWindows) { Join-Path $CoordinationRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin' }
        else { Split-Path -Parent (Get-Command rustc).Source }
}
$roundSuffix = if ($IsWindows) { '.exe' } else { '' }
$roundCargo = Join-Path $RustBin "cargo$roundSuffix"
$roundVersion = & (Join-Path $RustBin "rustc$roundSuffix") --version
if ($LASTEXITCODE -or $roundVersion -notmatch '^rustc 1\.99\.0 ') { throw 'Rust 1.99.0 is required.' }
$roundPins = Get-Content -Raw (Join-Path $PSScriptRoot 'round_hash_vectors/provenance.json') | ConvertFrom-Json
foreach ($roundPin in $roundPins.managed_reference.PSObject.Properties) {
    $roundText = [IO.File]::ReadAllText((Join-Path $roundRoot $roundPin.Name)).Replace("`r`n", "`n")
    $roundDigest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($roundText))).ToLowerInvariant()
    if ($roundDigest -ne $roundPin.Value) { throw "Independent managed reference changed: $($roundPin.Name)" }
}
if ((Get-Content -Raw (Join-Path $roundRoot 'mcw/src/lib.rs')) -notmatch 'pub mod round_hash;') {
    throw 'The bounded round-hash registration patch must be integrated in this isolated checkout before host verification.'
}
$roundSlot = $null
foreach ($roundIndex in 1..2) {
    try {
        $roundSlot = [IO.File]::Open((Join-Path $CoordinationRoot ".artifacts/mcw-coordination/build-slot-$roundIndex.lock"),
            [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        Write-Output "Build slot $roundIndex; verifier PID $PID; existing bounded client round fingerprint assignment."
        break
    } catch [IO.IOException] {}
}
if (-not $roundSlot) { throw 'Build slots are busy; defer verification without changing peers or processes.' }
$roundEnvironment = @('PATH','LIB','CARGO_HOME','CARGO_TARGET_DIR','CARGO_BUILD_JOBS','RUSTUP_HOME','RUSTUP_TOOLCHAIN','MCW_WINDOWS_RUNTIME')
$roundSaved = @{}
foreach ($roundVariable in $roundEnvironment) { $roundSaved[$roundVariable] = [Environment]::GetEnvironmentVariable($roundVariable, 'Process') }
$roundOutput = Join-Path $roundRoot ('.artifacts/round-hash-evidence/' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff'))
New-Item -ItemType Directory -Path $roundOutput -Force | Out-Null
Push-Location $roundRoot
try {
    if ($IsWindows -and (Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory * 1KB -lt 2GB) { throw 'Less than 2 GiB RAM is free.' }
    $env:PATH = $RustBin + [IO.Path]::PathSeparator + $env:PATH
    $env:CARGO_BUILD_JOBS = '1'
    $env:MCW_WINDOWS_RUNTIME = $null
    if ($IsWindows) {
        $env:CARGO_HOME = Join-Path $CoordinationRoot '.artifacts/mcw-tools/cargo'
        $env:RUSTUP_HOME = Join-Path $CoordinationRoot '.artifacts/mcw-tools/rustup'
        $env:RUSTUP_TOOLCHAIN = '1.99.0'
        $roundVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
        $roundVs = & $roundVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        & (Join-Path $roundVs 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation | Out-Null
        # This machine has the OneCore MSVC import libraries. They are test-tool
        # inputs; the shipping host below uses its audited first-party runtime.
        $roundOneCore = Join-Path $env:VCToolsInstallDir 'lib/onecore/x64'
        if (Test-Path -LiteralPath $roundOneCore) { $env:LIB = $roundOneCore + ';' + $env:LIB }
    }
    & $Python mcw/tests/round_hash_prepare_strobego.py --check
    if ($LASTEXITCODE) { throw 'Published STROBEgo fixture audit failed.' }
    & (Join-Path $RustBin "rustfmt$roundSuffix") --edition 2024 --config skip_children=true --check mcw/src/round_hash/mod.rs mcw/src/round_hash/strobe.rs mcw/tests/round_hash_conformance.rs mcw/tests/round_hash_strobe.inc
    if ($LASTEXITCODE) { throw 'Round-hash formatting check failed.' }
    dotnet build Contrib/Mcw/RoundHashProbe/RoundHashProbe.csproj -c Release -m:1 -p:RoundHashActivated=true -p:BuildMcwHost=false -p:UseSharedCompilation=false -p:RestoreLockedMode=true -nodeReuse:false --verbosity quiet
    if ($LASTEXITCODE) { throw 'Production managed caller/probe build failed.' }
    & dotnet Contrib/Mcw/RoundHashProbe/bin/Release/net10.0/RoundHashProbe.dll reference (Join-Path $roundOutput 'managed.tsv')
    if ($LASTEXITCODE) { throw 'Independent managed reference failed.' }
    $roundExpected = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'round_hash_vectors/managed.tsv')).Replace("`r`n", "`n")
    if ([IO.File]::ReadAllText((Join-Path $roundOutput 'managed.tsv')).Replace("`r`n", "`n") -cne $roundExpected) { throw 'Managed round-hash fixture bytes changed.' }
    $env:CARGO_TARGET_DIR = Join-Path $roundRoot '.artifacts/round-hash-evidence/test-target'
    & $roundCargo test --manifest-path mcw/Cargo.toml --locked --offline --lib round_hash -- --test-threads=1
    if ($LASTEXITCODE) { throw 'STROBEgo known-vector tests failed.' }
    & $roundCargo test --manifest-path mcw/Cargo.toml --locked --offline --test round_hash_conformance -- --test-threads=1
    if ($LASTEXITCODE) { throw 'Round hash conformance failed.' }
    & (Join-Path $RustBin "cargo-clippy$roundSuffix") clippy --manifest-path mcw/Cargo.toml --locked --offline --lib --bin mcw --test round_hash_conformance -- -D warnings
    if ($LASTEXITCODE) { throw 'Round-hash strict Clippy failed.' }
    $roundMetadata = & $roundCargo metadata --manifest-path mcw/Cargo.toml --locked --offline --format-version 1
    if ($LASTEXITCODE) { throw 'Cargo metadata audit failed.' }
    $roundGraph = ($roundMetadata -join "`n") | ConvertFrom-Json
    if ($roundGraph.packages.Count -ne 1 -or $roundGraph.packages[0].dependencies.Count) { throw 'External Cargo dependency introduced.' }
    $env:CARGO_TARGET_DIR = Join-Path $roundRoot '.artifacts/round-hash-evidence/native-target'
    if ($IsWindows) {
        & ./Contrib/Mcw/build-windows.ps1 -Cargo $roundCargo
        $roundBinary = Join-Path $env:CARGO_TARGET_DIR 'x86_64-pc-windows-msvc/release/mcw.exe'
    } else {
        & $roundCargo build --manifest-path mcw/Cargo.toml --release --locked --offline --bin mcw
        if ($LASTEXITCODE) { throw 'Release host build failed.' }
        $roundBinary = Join-Path $env:CARGO_TARGET_DIR 'release/mcw'
    }
    & $Python Contrib/Mcw/audit.py --binary $roundBinary
    if ($LASTEXITCODE) { throw 'Native runtime import audit failed.' }
    & $Python Contrib/Mcw/test-round-hash.py --binary $roundBinary --output (Join-Path $roundOutput 'actual-host')
    if ($LASTEXITCODE) { throw 'Actual-host synthetic caller tests failed.' }
    $roundSummary = [ordered]@{ compiler = $roundVersion; native_target = if ($IsWindows) { 'Windows x64' } else { 'local native target' };
        managed_reference_unchanged = $true; strobego_cases = 67; cargo_packages = 1; external_cargo_dependencies = 0;
        actual_host = Get-Content -Raw (Join-Path $roundOutput 'actual-host/summary.json') | ConvertFrom-Json;
        wallets = 0; live_coinjoin = $false; evidence = $roundOutput }
    $roundSummary | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $roundOutput 'summary.json') -Encoding utf8
    $roundSummary | ConvertTo-Json -Depth 10
} finally {
    Pop-Location
    foreach ($roundVariable in $roundEnvironment) { [Environment]::SetEnvironmentVariable($roundVariable, $roundSaved[$roundVariable], 'Process') }
    $roundSlot.Dispose()
}
