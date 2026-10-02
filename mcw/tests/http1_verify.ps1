param(
    [string]$SharedRoot = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',
    [string]$RustBin = ''
)
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'Use the portable rustc commands in the handoff on other OSes.' }
$componentRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if (-not $RustBin) { $RustBin = Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin' }
$rustc = Join-Path $RustBin 'rustc.exe'
$rustfmt = Join-Path $RustBin 'rustfmt.exe'
$clippy = Join-Path $RustBin 'clippy-driver.exe'
$rustVersion = (& $rustc --version) -join ''
if ($LASTEXITCODE -or $rustVersion -notmatch '^rustc 1\.99\.0 ') { throw 'Rust 1.99.0 required; no tools are installed by this verifier.' }
$availableGiB = [double](Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB
Write-Output ('Free memory GiB: {0:N2}' -f $availableGiB)
if ($availableGiB -lt 2) { throw 'Verification deferred: fewer than 2 GiB free.' }
$slotHandle = $null
foreach ($slot in 1..2) {
    try {
        $slotHandle = [IO.File]::Open((Join-Path $SharedRoot ".artifacts/mcw-coordination/build-slot-$slot.lock"), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        Write-Output "Build slot: $slot"
        break
    } catch [IO.IOException] { }
}
if (-not $slotHandle) { throw 'Verification deferred: both shared build slots occupied.' }
$previousJobs = $env:CARGO_BUILD_JOBS
$env:CARGO_BUILD_JOBS = '1'
try {
    if (-not $env:VCToolsInstallDir) {
        $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
        $vsRoot = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if (-not $vsRoot) { throw 'Installed native linker unavailable.' }
        & (Join-Path $vsRoot 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
    }
    $output = Join-Path $componentRoot '.artifacts/http1-verification'
    [IO.Directory]::CreateDirectory($output) | Out-Null
    $started = [DateTime]::UtcNow
    $source = Join-Path $componentRoot 'mcw/src/http1.rs'
    $conformance = Join-Path $PSScriptRoot 'http1_conformance.rs'
    $fixture = Join-Path $PSScriptRoot 'http1_fixture.rs'
    & $rustfmt --edition 2024 --check $source $conformance $fixture
    if ($LASTEXITCODE) { throw 'Formatting check failed.' }
    $harness = Join-Path $output 'lib.rs'
    [IO.File]::WriteAllText($harness, '#[path="../../mcw/src/http1.rs"] pub mod http1;' + "`n")
    & $rustc --edition=2024 --crate-name http1_component --crate-type=lib --emit=metadata -Dwarnings $harness -o (Join-Path $output 'http1.rmeta')
    if ($LASTEXITCODE) { throw 'Actual-source warning check failed.' }
    & $clippy --edition=2024 --crate-name http1_component --crate-type=lib --emit=metadata -Dwarnings -Dclippy::all $harness -o (Join-Path $output 'http1-clippy.rmeta')
    if ($LASTEXITCODE) { throw 'Actual-source Clippy check failed.' }
    foreach ($name in @('conformance', 'fixture')) {
        $testSource = Join-Path $PSScriptRoot "http1_$name.rs"
        $testBinary = Join-Path $output "http1_$name.exe"
        & $rustc --edition=2024 --crate-name "http1_$name" --test -Dwarnings $testSource -o $testBinary
        if ($LASTEXITCODE) { throw "$name test compilation failed." }
        & $testBinary --test-threads=1 --nocapture 2>&1 | Tee-Object -FilePath (Join-Path $output "$name-results.txt")
        if ($LASTEXITCODE) { throw "$name tests failed." }
    }
    # Optimized conformance exercises release arithmetic/control flow separately.
    $optimized = Join-Path $output 'http1_conformance_release.exe'
    & $rustc --edition=2024 --crate-name http1_conformance_release --test -O -Dwarnings $conformance -o $optimized
    if ($LASTEXITCODE) { throw 'Optimized conformance compilation failed.' }
    & $optimized --test-threads=1 --nocapture 2>&1 | Tee-Object -FilePath (Join-Path $output 'conformance-release-results.txt')
    if ($LASTEXITCODE) { throw 'Optimized conformance tests failed.' }
    $hashes = @{}
    foreach ($file in @($source, $conformance, $fixture, $PSCommandPath)) {
        $hashes[[IO.Path]::GetRelativePath($componentRoot, $file).Replace('\','/')] = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    [ordered]@{
        scope = 'HTTP/1 domain module and synthetic loopback fixtures only'
        rust = $rustVersion
        edition = 2024
        native_test_target = 'x86_64-pc-windows-msvc'
        started_utc = $started.ToString('O')
        completed_utc = [DateTime]::UtcNow.ToString('O')
        format = 'passed'
        warnings = 'denied; passed'
        clippy = 'all denied; passed'
        test_logs = @('conformance-results.txt', 'fixture-results.txt', 'conformance-release-results.txt')
        source_sha256 = $hashes
        external_crates = 0
        shipping_cargo_manifests_modified = $false
        production_callers_integrated = $false
        five_target_acceptance = 'not run; host-owned integration/packaging still required'
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $output 'verification.json') -Encoding utf8
    Write-Output ('Verified component: ' + (Join-Path $output 'verification.json'))
} finally {
    $env:CARGO_BUILD_JOBS = $previousJobs
    $slotHandle.Dispose()
}
