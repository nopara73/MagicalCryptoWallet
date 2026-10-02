param([string]$Cargo = 'cargo', [string]$Version = '99.99.99', [switch]$PrepareStandardLibrary)
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'This script requires PowerShell 7 on Windows x64.' }
$taskRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$manifest = Join-Path $taskRoot 'mcw/Cargo.toml'
if (-not $env:VCToolsInstallDir) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $vsRoot = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $vsRoot) { throw 'Install Visual Studio C++ build tools and a Windows SDK.' }
    & (Join-Path $vsRoot 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
}
$cargoCommand = (Get-Command $Cargo -ErrorAction Stop).Source
$variables = @('RUSTC_BOOTSTRAP','RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','MCW_WINDOWS_RUNTIME','MCW_VERSION','CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER')
$previous = @{}
foreach ($variable in $variables) { $previous[$variable] = [Environment]::GetEnvironmentVariable($variable, 'Process') }
try {
    $compiler = Join-Path (Split-Path -Parent $cargoCommand) 'rustc.exe'
    $versionText = & $compiler --version
    if ($LASTEXITCODE -or $versionText -notmatch '^rustc 1\.99\.0 ') { throw 'mcw requires Rust 1.99.0.' }
    $metadata = & $cargoCommand metadata --manifest-path $manifest --locked --offline --format-version 1
    if ($LASTEXITCODE) { throw 'Cargo metadata failed.' }
    $graph = ($metadata -join "`n") | ConvertFrom-Json
    if ($graph.packages.Count -ne 1 -or $graph.packages[0].dependencies.Count) { throw 'External Cargo dependencies are prohibited.' }
    # Only the matching standard library is rebuilt; the application graph stays
    # empty. Build-std is unstable, so bootstrap is scoped to this exact compiler.
    $env:RUSTC_BOOTSTRAP = '1'
    $env:RUSTFLAGS = '-C panic=abort -C default-linker-libraries=no'
    $env:CARGO_ENCODED_RUSTFLAGS = $null
    $env:MCW_WINDOWS_RUNTIME = '1'
    $env:MCW_VERSION = $Version
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = (Get-Command link.exe).Source
    $arguments = @('-Z','build-std=std,panic_abort','-Z','build-std-features=backtrace','build','--manifest-path',$manifest,'--release','--target','x86_64-pc-windows-msvc','--locked','--bin','mcw')
    if (-not $PrepareStandardLibrary) { $arguments += '--offline' }
    & $cargoCommand @arguments
    if ($LASTEXITCODE) { throw 'mcw Windows runtime build failed.' }
    $targetPath = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $taskRoot 'mcw/target' }
    $binary = Join-Path $targetPath 'x86_64-pc-windows-msvc/release/mcw.exe'
    $imports = & dumpbin.exe /nologo /imports $binary
    if ($LASTEXITCODE) { throw 'PE import audit failed.' }
    if (($imports -join "`n") -match '(?i)(vcruntime|msvcp\d|msvcr\d|libgcc|libstdc\+\+|libc\+\+)') { throw 'mcw acquired a non-OS runtime import.' }
    Write-Output $binary
} finally {
    foreach ($variable in $variables) { [Environment]::SetEnvironmentVariable($variable, $previous[$variable], 'Process') }
}
