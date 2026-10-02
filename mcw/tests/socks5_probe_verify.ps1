param([string]$SharedRoot = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet')
$ErrorActionPreference = 'Stop'
$taskRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$outputRoot = Join-Path $taskRoot '.artifacts/socks5-probe'
[IO.Directory]::CreateDirectory($outputRoot) | Out-Null
$slotHandle = $null
$freeGiB = [double](Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB
if ($freeGiB -lt 2) { throw 'Verification deferred: less than 2 GiB free.' }
foreach ($slot in 1..2) {
    try { $slotHandle = [IO.File]::Open((Join-Path $SharedRoot ".artifacts/mcw-coordination/build-slot-$slot.lock"),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None); break }
    catch [IO.IOException] { }
}
if (-not $slotHandle) { throw 'Verification deferred: shared build slots busy.' }
$variables = @('PATH','CARGO_HOME','RUSTUP_HOME','CARGO_TARGET_DIR','CARGO_BUILD_JOBS')
$previous = @{}
foreach ($variable in $variables) { $previous[$variable] = [Environment]::GetEnvironmentVariable($variable,'Process') }
try {
    $toolsRoot = Join-Path $SharedRoot '.artifacts/mcw-tools'
    $rustBin = Join-Path $toolsRoot 'rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin'
    $env:PATH = $rustBin + ';' + $env:PATH
    $env:CARGO_HOME = Join-Path $toolsRoot 'cargo'
    $env:RUSTUP_HOME = Join-Path $toolsRoot 'rustup'
    $env:CARGO_TARGET_DIR = Join-Path $outputRoot 'target'
    $env:CARGO_BUILD_JOBS = '1'
    & (Join-Path $taskRoot 'Contrib/Mcw/build-windows.ps1') -Cargo (Join-Path $rustBin 'cargo.exe') 2>&1 | Tee-Object -FilePath (Join-Path $outputRoot 'native-build.txt')
    if ($LASTEXITCODE) { throw 'Actual host build failed.' }
    $binary = Join-Path $env:CARGO_TARGET_DIR 'x86_64-pc-windows-msvc/release/mcw.exe'
    if (-not (Test-Path -LiteralPath $binary)) { throw 'Native host binary is missing.' }
    & (Join-Path $rustBin 'rustfmt.exe') --edition 2024 --check (Join-Path $taskRoot 'mcw/src/socks5.rs') (Join-Path $taskRoot 'mcw/tests/socks5_probe_service.rs')
    if ($LASTEXITCODE) { throw 'Formatting check failed.' }
    $harness = Join-Path $outputRoot 'lib.rs'
    [IO.File]::WriteAllText($harness,'#[path="../../mcw/src/socks5.rs"] pub mod socks5;' + "`n")
    & (Join-Path $rustBin 'clippy-driver.exe') --edition=2024 --crate-name socks5_probe --crate-type=lib --emit=metadata -Dwarnings -Dclippy::all $harness -o (Join-Path $outputRoot 'probe.rmeta')
    if ($LASTEXITCODE) { throw 'Strict module lint failed.' }
    foreach ($name in @('wire','transport','probe_service')) {
        $testBinary = Join-Path $outputRoot "socks5_$name.exe"
        & (Join-Path $rustBin 'rustc.exe') --edition=2024 --test -Dwarnings (Join-Path $PSScriptRoot "socks5_$name.rs") -o $testBinary
        if ($LASTEXITCODE) { throw "$name test compilation failed." }
        & $testBinary --test-threads=1 2>&1 | Tee-Object -FilePath (Join-Path $outputRoot "$name-tests.txt")
        if ($LASTEXITCODE) { throw "$name tests failed." }
    }
    if ([double](Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB -lt 2) { throw 'Managed verification deferred: less than 2 GiB free.' }
    $managedRoot = Join-Path $outputRoot 'ManagedProbe'
    [IO.Directory]::CreateDirectory($managedRoot) | Out-Null
    $project = @'
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup><OutputType>Exe</OutputType><AssemblyName>SocksProbe</AssemblyName><DisableImplicitNamespaceImports>true</DisableImplicitNamespaceImports></PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../../../MagicalCryptoWallet/MagicalCryptoWallet.csproj" />
    <Compile Include="../../../MagicalCryptoWallet.Client/Application/ManagedApplicationHost.cs" Link="ManagedApplicationHost.cs" />
    <Compile Include="../../../mcw/tests/socks5_probe_caller.cs" Link="Program.cs" />
  </ItemGroup>
</Project>
'@
    [IO.File]::WriteAllText((Join-Path $managedRoot 'SocksProbe.csproj'),$project)
    & dotnet build (Join-Path $managedRoot 'SocksProbe.csproj') -c Release -m:1 -p:UseSharedCompilation=false -p:NuGetAudit=false -p:CopyToOutputDirectory=Never 2>&1 | Tee-Object -FilePath (Join-Path $outputRoot 'managed-build.txt')
    if ($LASTEXITCODE) { throw 'Real managed caller compilation failed.' }
    $runRoot = Join-Path $outputRoot 'runtime'
    [IO.Directory]::CreateDirectory($runRoot) | Out-Null
    Copy-Item -Path (Join-Path $managedRoot 'bin/Release/net10.0/*') -Destination $runRoot -Recurse -Force
    Copy-Item -LiteralPath $binary -Destination (Join-Path $runRoot 'mcw.exe') -Force
    Copy-Item -LiteralPath (Join-Path $runRoot 'SocksProbe.exe') -Destination (Join-Path $runRoot 'magicalcryptowallet.exe') -Force
    $report = Join-Path $runRoot 'results.json'
    $errorLog = Join-Path $outputRoot 'caller-stderr.txt'
    $outputLog = Join-Path $outputRoot 'caller-stdout.txt'
    $runArguments = @('gui','socks-probe-tests',$report)
    $child = Start-Process -FilePath (Join-Path $runRoot 'mcw.exe') -ArgumentList $runArguments -WindowStyle Hidden -PassThru -RedirectStandardError $errorLog -RedirectStandardOutput $outputLog
    if (-not $child.WaitForExit(60000)) { throw 'Synthetic probe test exceeded 60 seconds; inspect the owned test process.' }
    if ($child.ExitCode -ne 0) { Get-Content -LiteralPath $errorLog; throw ('Synthetic production caller failed: ' + $child.ExitCode) }
    $result = Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
    if ($result.checks -lt 25 -or -not $result.noTorLaunched -or -not $result.noWalletData) { throw 'Production caller evidence is incomplete.' }
    $diagnostics = [IO.File]::ReadAllText($errorLog) + [IO.File]::ReadAllText($outputLog)
    if ($diagnostics -match '127\.0\.0\.1|example\.invalid|192\.0\.2\.1|synthetic-isolation') { throw 'Sensitive endpoint/payload appeared in diagnostics.' }
    & dumpbin.exe /nologo /dependents $binary | Tee-Object -FilePath (Join-Path $outputRoot 'runtime-imports.txt')
    if ($LASTEXITCODE) { throw 'Runtime audit failed.' }
    $result | Add-Member -NotePropertyName native_binary_sha256 -NotePropertyValue (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant()
    $result | Add-Member -NotePropertyName verified_utc -NotePropertyValue ([DateTime]::UtcNow.ToString('O'))
    $result | Add-Member -NotePropertyName pinned_base -NotePropertyValue '7ae424b5f5f3734ca1870962a2d913769c59b26d'
    $result | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $outputRoot 'verification.json') -Encoding utf8
    Write-Output ('VERIFIED_PROBE_CALLER=' + (Join-Path $outputRoot 'verification.json'))
} finally {
    foreach ($variable in $variables) { [Environment]::SetEnvironmentVariable($variable,$previous[$variable],'Process') }
    $slotHandle.Dispose()
}
