param(
    [string]$SharedRoot = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet',
    [string]$Rustc = '',
    [string]$Mode = 'portable'
)
$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$evidenceRoot = Join-Path $repoRoot ('.artifacts/qr-scanning/' + $Mode)
New-Item -ItemType Directory -Force -Path $evidenceRoot | Out-Null
if (-not $Rustc) { $Rustc = Join-Path $SharedRoot '.artifacts/mcw-tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin/rustc.exe' }
$buildHandle = $null
$previousPath = $env:PATH
$previousLib = $env:LIB
try {
    if ((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory -lt 2097152) { throw 'Build deferred: less than 2 GiB free memory' }
    foreach ($slot in 1, 2) {
        try {
            $buildHandle = [IO.File]::Open((Join-Path $SharedRoot ('.artifacts/mcw-coordination/build-slot-' + $slot + '.lock')),
                [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
            Write-Host ('QR scanner conformance acquired build-slot-' + $slot + ', verifier PID ' + $PID)
            break
        } catch [IO.IOException] { }
    }
    if (-not $buildHandle) { throw 'Build deferred: both compiler slots occupied' }
    $version = & $Rustc --version
    if ($LASTEXITCODE -ne 0 -or $version -notlike 'rustc 1.99.0 *') { throw 'Rust 1.99.0 required' }
    $linker = Get-ChildItem -Path 'C:\Program Files\Microsoft Visual Studio\*\*\VC\Tools\MSVC\*\bin\Hostx64\x64\link.exe' |
        Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $linker) { throw 'Existing MSVC linker required' }
    $msvcRoot = [IO.Path]::GetFullPath((Join-Path $linker.Directory.FullName '../../..'))
    $sdk = Get-ChildItem -LiteralPath 'C:\Program Files (x86)\Windows Kits\10\Lib' -Directory | Sort-Object Name -Descending | Select-Object -First 1
    $env:PATH = $linker.Directory.FullName + ';' + $env:PATH
    $env:LIB = (Join-Path $msvcRoot 'lib/onecore/x64') + ';' + (Join-Path $sdk.FullName 'ucrt/x64') + ';' + (Join-Path $sdk.FullName 'um/x64')
    $tests = Join-Path $repoRoot 'mcw/tests/qr_scanning_conformance.rs'
    $clippy = Join-Path (Split-Path $Rustc) 'clippy-driver.exe'
    & $clippy '--edition=2024' '--test' '-D' 'warnings' '--emit=metadata' $tests '-o' (Join-Path $evidenceRoot 'conformance.rmeta')
    if ($LASTEXITCODE -ne 0) { throw 'Strict portable Clippy failed' }
    $runs=@()
    foreach ($profile in 'debug','optimized') {
        $exe = Join-Path $evidenceRoot ('conformance-' + $profile + '.exe')
        # This is a non-shipping reference harness. Production uses QR host's
        # rebuilt-stdlib/native-runtime path; static CRT is forbidden there.
        $argsForRust = @('--edition=2024','--test','-D','warnings','-C','codegen-units=1','-C','overflow-checks=yes',
            '-C','target-feature=+crt-static',$tests,'-o',$exe)
        if ($profile -eq 'optimized') { $argsForRust += @('-C','opt-level=2') }
        & $Rustc @argsForRust
        if ($LASTEXITCODE -ne 0) { throw ($profile + ' build failed') }
        $log=Join-Path $evidenceRoot ($profile+'.log')
        & $exe --test-threads=1 2>&1 | Tee-Object -FilePath $log
        if ($LASTEXITCODE -ne 0) { throw ($profile + ' conformance failed') }
        $runs += @{profile=$profile;log=$log}
    }
    & $Rustc '--edition=2024' '-D' 'warnings' '-C' 'codegen-units=1' '-C' 'overflow-checks=yes' '-C' 'target-feature=+crt-static' '-C' 'opt-level=2' (Join-Path $repoRoot 'mcw/tests/qr_scanning_tools/oracle.rs') '-o' (Join-Path $evidenceRoot 'oracle.exe')
    if ($LASTEXITCODE -ne 0) { throw 'Non-shipping oracle driver build failed' }
    $hashes=@{}
    Get-ChildItem -LiteralPath (Join-Path $repoRoot 'mcw/src/scan_service') -Filter '*.rs' | ForEach-Object {
        $hashes[$_.Name]=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    @{rust=$version;source_hashes=$hashes;runs=$runs;production_package_verified=$false;camera_hardware_verified=$false} |
        ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $evidenceRoot 'result.json')
} finally {
    $env:PATH=$previousPath; $env:LIB=$previousLib
    if ($buildHandle) { $buildHandle.Dispose() }
}
