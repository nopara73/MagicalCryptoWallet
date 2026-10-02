param(
    [string]$BitcoinEncodingPath,
    [string]$RustBin = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet\.artifacts\mcw-tools\rustup\toolchains\1.99.0-x86_64-pc-windows-msvc\bin',
    [string]$CoordinationRoot = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet\.artifacts\mcw-coordination',
    [string]$Python = 'python',
    # For a shell outside a VS developer environment, pass native MSVC/SDK
    # library directories explicitly. This configures only this test process.
    [string[]]$NativeLibraryPaths = @(),
    [ValidateRange(0, 50)] [int]$BuildSlotWaitSeconds = 0,
    [switch]$CargoHost,
    [switch]$Optimized
)
$ErrorActionPreference = 'Stop'
$uriCheckout = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
if (-not $BitcoinEncodingPath) {
    $BitcoinEncodingPath = Join-Path $uriCheckout 'mcw\src\bitcoin_encoding.rs'
}
$BitcoinEncodingPath = (Resolve-Path -LiteralPath $BitcoinEncodingPath).Path
$uriOutput = Join-Path $uriCheckout '.artifacts\payment-uri-evidence'
New-Item -ItemType Directory -Force -Path $uriOutput | Out-Null
$uriBuildLock = $null
$uriFreeGiB = (Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB
if ($uriFreeGiB -lt 2) { Write-Output 'DEFERRED_LOW_MEMORY'; exit 3 }
$uriSlotTimer = [Diagnostics.Stopwatch]::StartNew()
do {
    foreach ($uriSlot in 1..2) {
        try {
            $uriBuildLock = [IO.File]::Open((Join-Path $CoordinationRoot "build-slot-$uriSlot.lock"), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
            break
        } catch [IO.IOException] { }
    }
    if ($null -ne $uriBuildLock -or $uriSlotTimer.Elapsed.TotalSeconds -ge $BuildSlotWaitSeconds) { break }
    Start-Sleep -Seconds 2
} while ($true)
if ($null -eq $uriBuildLock) { Write-Output 'DEFERRED_BUILD_SLOTS_BUSY'; exit 3 }
try {
    if ((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB -lt 2) {
        Write-Output 'DEFERRED_LOW_MEMORY'; exit 3
    }
    $env:CARGO_BUILD_JOBS = '1'
    foreach ($uriNativeLib in $NativeLibraryPaths) {
        if (-not (Test-Path -LiteralPath $uriNativeLib -PathType Container)) {
            throw "Native build library directory is missing: $uriNativeLib"
        }
    }
    if ($NativeLibraryPaths.Count -ne 0) {
        $env:LIB = ($NativeLibraryPaths + @($env:LIB) | Where-Object { $_ }) -join ';'
    }
    $uriOptimization = if ($Optimized) { @('-C', 'opt-level=3') } else { @() }
    # Snapshot an actual first-party module in this checkout. No peer file is
    # edited, and the source hash is recorded so preliminary source is explicit.
    $uriEncodingSnapshot = Join-Path $uriOutput 'bitcoin_encoding.reference.rs'
    Copy-Item -LiteralPath $BitcoinEncodingPath -Destination $uriEncodingSnapshot
    $uriQuote = { param([string]$path) ConvertTo-Json -InputObject $path -Compress }
    $uriEncodingLiteral = & $uriQuote $uriEncodingSnapshot
    $uriSourceLiteral = & $uriQuote (Join-Path $uriCheckout 'mcw\src\payment_uri.rs')
    $uriSemanticsLiteral = & $uriQuote (Join-Path $PSScriptRoot 'payment_uri_semantics.rs')
    $uriAddressLiteral = & $uriQuote (Join-Path $PSScriptRoot 'payment_uri_addresses.rs')
    $uriServiceLiteral = & $uriQuote (Join-Path $PSScriptRoot 'payment_uri_service.rs')
    $uriHarness = @"
extern crate self as mcw;
#[path = $uriEncodingLiteral] pub mod bitcoin_encoding;
#[path = $uriSourceLiteral] pub mod payment_uri;
#[path = $uriSemanticsLiteral] mod payment_uri_semantics;
#[path = $uriAddressLiteral] mod payment_uri_addresses;
#[path = $uriServiceLiteral] mod payment_uri_service;
"@
    $uriHarnessPath = Join-Path $uriOutput 'test_harness.rs'
    [IO.File]::WriteAllText($uriHarnessPath, $uriHarness, [Text.UTF8Encoding]::new($false))
    $uriTestsExe = Join-Path $uriOutput 'payment_uri_tests.exe'
    & (Join-Path $RustBin 'rustfmt.exe') --edition 2024 --check (Join-Path $uriCheckout 'mcw\src\payment_uri.rs') (Join-Path $PSScriptRoot 'payment_uri_semantics.rs') (Join-Path $PSScriptRoot 'payment_uri_addresses.rs') (Join-Path $PSScriptRoot 'payment_uri_service.rs')
    if ($LASTEXITCODE -ne 0) { throw 'Rustfmt verification failed' }
    & (Join-Path $RustBin 'clippy-driver.exe') --edition=2024 --test --emit=metadata -W clippy::all -D warnings $uriHarnessPath -o (Join-Path $uriOutput 'clippy.rmeta')
    if ($LASTEXITCODE -ne 0) { throw 'Clippy verification failed' }
    & (Join-Path $RustBin 'rustc.exe') --edition=2024 --test -D warnings -C target-feature=+crt-static @uriOptimization $uriHarnessPath -o $uriTestsExe
    if ($LASTEXITCODE -ne 0) { throw 'Rust test harness compilation failed' }
    $uriTestOutput = @(& $uriTestsExe --test-threads=1)
    $uriTestExit = $LASTEXITCODE
    $uriTestOutput | Set-Content -LiteralPath (Join-Path $uriOutput 'rust-tests.txt') -Encoding utf8
    $uriTestOutput | Write-Output
    if ($uriTestExit -ne 0) { throw 'Rust tests failed' }
    $uriOracle = @'
use std::io::{self, BufRead};
#[path = @ENCODING@] pub mod bitcoin_encoding;
#[path = @PAYMENT@] pub mod payment_uri;
fn main() {
    for line in io::stdin().lock().lines() {
        let line = line.expect("synthetic input");
        let (kind, hex) = line.split_once('\t').expect("synthetic protocol");
        let bytes = bitcoin_encoding::hex_decode(hex).expect("synthetic hex");
        if kind == "H" {
            let operation = u16::from_le_bytes(bytes[..2].try_into().expect("synthetic operation"));
            match payment_uri::service::handle(operation, &bytes[2..]) {
                Ok(reply) => println!("OK:{}", bitcoin_encoding::hex_encode(&reply).expect("bounded reply")),
                Err(error) => println!("ERR:{}", error.code),
            }
            continue;
        }
        let text = String::from_utf8(bytes).expect("synthetic UTF-8");
        let result = match kind {
            "A" => payment_uri::Amount::parse_btc(&text).map(|a| a.satoshis().to_string()).ok(),
            "S" => text.parse::<u64>().ok().and_then(|n| payment_uri::Amount::from_satoshis(n).ok()).map(|a| a.to_btc()),
            "E" => payment_uri::percent_encode(&text).ok().and_then(|s| bitcoin_encoding::hex_encode(s.as_bytes()).ok()),
            "D" | "F" => payment_uri::percent_decode(&text, if kind == "F" { payment_uri::ParsingMode::ManagedCompatibility } else { payment_uri::ParsingMode::Bip21 }).ok().and_then(|s| bitcoin_encoding::hex_encode(s.as_bytes()).ok()),
            _ => panic!("synthetic operation"),
        };
        println!("{}", result.as_deref().unwrap_or("ERR"));
    }
}
'@
    $uriOracle = $uriOracle.Replace('@ENCODING@', $uriEncodingLiteral).Replace('@PAYMENT@', $uriSourceLiteral)
    $uriOraclePath = Join-Path $uriOutput 'reference_harness.rs'
    [IO.File]::WriteAllText($uriOraclePath, $uriOracle, [Text.UTF8Encoding]::new($false))
    $uriOracleExe = Join-Path $uriOutput 'payment_uri_reference.exe'
    & (Join-Path $RustBin 'rustc.exe') --edition=2024 -D warnings -C target-feature=+crt-static @uriOptimization $uriOraclePath -o $uriOracleExe
    if ($LASTEXITCODE -ne 0) { throw 'Rust reference harness compilation failed' }
    & $Python (Join-Path $PSScriptRoot 'payment_uri_reference.py') --executable $uriOracleExe --evidence (Join-Path $uriOutput 'reference-results.json')
    if ($LASTEXITCODE -ne 0) { throw 'Independent Decimal/urllib verification failed' }
    if ($CargoHost) {
        # Verify module declarations and the actual application dependency graph,
        # without changing the QR-owned manifest, host or custom release runtime.
        $env:RUSTC = Join-Path $RustBin 'rustc.exe'
        $env:CARGO_TARGET_DIR = Join-Path $uriCheckout '.artifacts\payment-uri-cargo'
        $env:RUSTFLAGS = '-C target-feature=+crt-static'
        Remove-Item Env:\MCW_WINDOWS_RUNTIME -ErrorAction SilentlyContinue
        $uriManifest = Join-Path $uriCheckout 'mcw\Cargo.toml'
        $uriCargo = Join-Path $RustBin 'cargo.exe'
        $uriMetadata = & $uriCargo metadata --manifest-path $uriManifest --offline --locked --format-version 1
        if ($LASTEXITCODE -ne 0) { throw 'Cargo host metadata failed' }
        $uriMetadata | Set-Content -LiteralPath (Join-Path $uriOutput 'cargo-metadata.json') -Encoding utf8
        $uriGraph = $uriMetadata | ConvertFrom-Json
        if ($uriGraph.packages.Count -ne 1 -or $uriGraph.packages[0].dependencies.Count -ne 0) {
            throw 'Cargo host must contain one package and no dependencies'
        }
        $uriCargoMode = if ($Optimized) { @('--release') } else { @() }
        & $uriCargo test --manifest-path $uriManifest --offline --locked @uriCargoMode --test payment_uri_semantics --test payment_uri_addresses --test payment_uri_service -- --test-threads=1 2>&1 |
            Tee-Object -FilePath (Join-Path $uriOutput 'cargo-tests.txt')
        if ($LASTEXITCODE -ne 0) { throw 'Actual Cargo host payment URI tests failed' }
    }
    $uriEvidence = [ordered]@{
        bitcoin_encoding_source = $BitcoinEncodingPath
        bitcoin_encoding_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $uriEncodingSnapshot).Hash
        payment_uri_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $uriCheckout 'mcw\src\payment_uri.rs')).Hash
        payment_uri_service_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $uriCheckout 'mcw\src\payment_uri\service.rs')).Hash
        rustc = (& (Join-Path $RustBin 'rustc.exe') --version)
        host = 'x86_64-pc-windows-msvc'
        optimized = [bool]$Optimized
        cargo_host = [bool]$CargoHost
        clippy_all_warnings_denied = $true
        passed = $true
    }
    if ($CargoHost) {
        $uriEvidence.cargo_verification_base = (& git -C $uriCheckout rev-parse HEAD)
        $uriEvidence.cargo_packages = $uriGraph.packages.Count
        $uriEvidence.cargo_dependencies = $uriGraph.packages[0].dependencies.Count
        $uriEvidence.cargo_test_targets = @('payment_uri_semantics', 'payment_uri_addresses', 'payment_uri_service')
        $uriEvidence.production_routing_verified = $false
        $uriEvidence.native_release_verified = $false
    }
    $uriEvidence | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $uriOutput 'verification.json') -Encoding utf8
    $uriEvidence | ConvertTo-Json | Write-Output
} finally {
    $uriBuildLock.Dispose()
}
