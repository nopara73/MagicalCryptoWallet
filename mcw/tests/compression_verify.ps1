param(
    [string]$SharedRoot = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',
    [string]$Rustc = '',
    [string]$Python = 'C:\Python314\python.exe'
)
$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$evidenceRoot = Join-Path $repoRoot '.artifacts/compression'
New-Item -ItemType Directory -Force -Path $evidenceRoot | Out-Null
if (-not $Rustc) { $Rustc = Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin/rustc.exe' }
$source = Join-Path $repoRoot 'mcw/src/compression.rs'
$tests = Join-Path $repoRoot 'mcw/tests/compression_conformance.rs'
$reference = Join-Path $repoRoot 'mcw/tests/compression_reference.py'
function Source-Hash([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
$hashes = @{ implementation = Source-Hash $source; conformance = Source-Hash $tests; reference = Source-Hash $reference }
$verifierHash = Source-Hash $PSCommandPath
$buildHandle = $null
$previousPath = $env:PATH
$previousLib = $env:LIB
try {
    if ((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory -lt 2097152) { throw 'Build deferred: less than 2 GiB free memory' }
    foreach ($slot in 1, 2) {
        try {
            $buildHandle = [IO.File]::Open((Join-Path $SharedRoot ('.artifacts/mcw-coordination/build-slot-' + $slot + '.lock')),
                [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
            break
        } catch [IO.IOException] { }
    }
    if (-not $buildHandle) { throw 'Build deferred: both compiler slots occupied' }
    Write-Output ('BUILD SLOT '+$slot+'; verifier PID '+$PID+'; existing compression regression verification')
    $version = & $Rustc --version
    if ($LASTEXITCODE -ne 0 -or $version -notlike 'rustc 1.99.0 *') { throw 'Rust 1.99.0 required' }
    $linker = Get-ChildItem -Path 'C:\Program Files\Microsoft Visual Studio\*\*\VC\Tools\MSVC\*\bin\Hostx64\x64\link.exe' |
        Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $linker) { throw 'Existing MSVC linker required' }
    $msvcRoot = [IO.Path]::GetFullPath((Join-Path $linker.Directory.FullName '../../..'))
    $sdk = Get-ChildItem -LiteralPath 'C:\Program Files (x86)\Windows Kits\10\Lib' -Directory | Sort-Object Name -Descending | Select-Object -First 1
    $env:PATH = $linker.Directory.FullName + ';' + $env:PATH
    $env:LIB = (Join-Path $msvcRoot 'lib/onecore/x64') + ';' + (Join-Path $sdk.FullName 'ucrt/x64') + ';' + (Join-Path $sdk.FullName 'um/x64')
    $runs = @()
    foreach ($mode in 'debug', 'optimized') {
        $exe = Join-Path $evidenceRoot ('conformance-' + $mode + '.exe')
        $argsForRust = @('--edition=2024','--test','-D','warnings','-C','codegen-units=1','-C','overflow-checks=yes',
            '-C','target-feature=+crt-static',$tests,'-o',$exe)
        if ($mode -eq 'optimized') { $argsForRust += @('-C','opt-level=2') }
        & $Rustc @argsForRust
        if ($LASTEXITCODE -ne 0) { throw ($mode + ' build failed') }
        $log = Join-Path $evidenceRoot ($mode + '.log')
        & $exe --test-threads=1 2>&1 | Tee-Object -FilePath $log
        if ($LASTEXITCODE -ne 0) { throw ($mode + ' conformance failed') }
        $text = Get-Content -LiteralPath $log -Raw
        if ($text -notmatch 'test result: ok\. (\d+) passed; 0 failed;') { throw 'Test completion not verified' }
        $runs += @{ mode = $mode; passed = [int]$Matches[1]; failed = 0; log = $log; log_sha256 = Source-Hash $log }
    }
    # This generated tool includes the actual assigned source, without stubs,
    # external Cargo packages or a second shipped executable.
    $driverText = @'
#![forbid(unsafe_code)]
#[allow(dead_code)]
#[path = "SOURCE_PATH"] mod compression;
use compression::*;
use std::io::{self, BufRead};
fn unhex(s: &str) -> Vec<u8> {
    if s == "-" { return Vec::new(); }
    s.as_bytes().chunks_exact(2).map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(),16).unwrap()).collect()
}
fn hex(b: &[u8]) -> String { b.iter().map(|v| format!("{v:02x}")).collect() }
fn run(v: &[&str]) -> Result<(Vec<u8>,usize,u32),Error> {
    let f=match v[1] {"raw"=>Format::Deflate,"zlib"=>Format::Zlib,"gzip"=>Format::Gzip,_=>panic!("format")};
    let bytes=unhex(v[3]); let dict=unhex(v[4]);
    if v[0]=="E" {
        let mut o=EncodeOptions::new(f); o.method=if v[2]=="stored" {EncodeMethod::Stored} else {EncodeMethod::Fixed};
        return encode(&bytes,o).map(|b|(b,bytes.len(),1));
    }
    let mut o=DecodeOptions::new(f);
    if v[4]!="-" {o.dictionary=Dictionary::Use(&dict);}
    if v[5]=="allow" {o.trailing_data=TrailingData::Allow;}
    if v[6]=="first" {o.gzip_members=GzipMembers::First;}
    if v[0]=="D" { return decode(&bytes,o).map(|d|(d.bytes,d.consumed,d.members)); }
    let chunks:Vec<usize>=v[2].split(':').map(|s|s.parse().unwrap()).collect();
    let mut decoder=Decoder::new(o)?; let mut pos=0; let mut fed=chunks[0].min(bytes.len()); let mut result=Vec::new();
    loop {
        let mut output=vec![0;chunks[1]];
        let progress=decoder.process(&bytes[pos..fed],&mut output,fed==bytes.len())?;
        pos+=progress.consumed; result.extend_from_slice(&output[..progress.written]);
        match progress.status {
            Status::Finished=>return Ok((result,pos,decoder.members())),
            Status::NeedInput=>{assert!(fed<bytes.len()); fed=(fed+chunks[0]).min(bytes.len());},
            Status::NeedOutput=>assert!(chunks[1]>0),
        }
    }
}
fn main() {
    for line in io::stdin().lock().lines() {
        let line=line.unwrap(); let v:Vec<_>=line.split('\t').collect();
        match run(&v) {
            Ok((b,n,m))=>println!("OK\t{n}\t{m}\t{}",hex(&b)),
            Err(e)=>println!("ERR\t{:?}\t{}\t{}",e.kind,e.input_consumed,e.output_produced),
        }
    }
}
'@
    $driverText = $driverText.Replace('SOURCE_PATH',$source.Replace('\','/'))
    $driverSource = Join-Path $evidenceRoot 'actual-source-driver.rs'
    $driverExe = Join-Path $evidenceRoot 'actual-source-driver.exe'
    [IO.File]::WriteAllText($driverSource,$driverText)
    & $Rustc --edition=2024 -D warnings -C codegen-units=1 -C overflow-checks=yes -C opt-level=2 -C target-feature=+crt-static $driverSource -o $driverExe
    if ($LASTEXITCODE -ne 0) { throw 'Differential driver build failed' }
    & $Python $reference --driver $driverExe --evidence $evidenceRoot
    if ($LASTEXITCODE -ne 0) { throw 'Independent differential verification failed' }
    if ((Source-Hash $source) -ne $hashes.implementation -or (Source-Hash $tests) -ne $hashes.conformance -or
        (Source-Hash $reference) -ne $hashes.reference -or (Source-Hash $PSCommandPath) -ne $verifierHash) { throw 'Sources changed during verification' }
    $evidence = @{ compiler=$version; edition=2024; platform='x86_64-pc-windows-msvc'; sources=$hashes; verifier_sha256=$verifierHash; runs=$runs;
        differential=(Get-Content -LiteralPath (Join-Path $evidenceRoot 'differential.json') -Raw | ConvertFrom-Json);
        driver_source=$driverSource; driver_sha256=(Source-Hash $driverSource); production_integrated=$false;
        note='Actual-source ignored test harness only. No Cargo package, shipping executable, native compression library or production adapter added.' }
    $evidence | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $evidenceRoot 'verification.json') -Encoding utf8
    $evidence | ConvertTo-Json -Depth 10
} finally {
    $env:PATH = $previousPath
    $env:LIB = $previousLib
    if ($buildHandle) { $buildHandle.Dispose();Write-Output ('BUILD SLOT '+$slot+' released; verifier PID '+$PID) }
}
