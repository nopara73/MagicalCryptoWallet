$ErrorActionPreference = 'Stop'
$taskRoot = (Resolve-Path "$PSScriptRoot/../..").Path
$taskWork = Join-Path $taskRoot '.artifacts/installed-coexistence'
[IO.Directory]::CreateDirectory($taskWork) | Out-Null
$taskReports = Join-Path $taskRoot '.artifacts/package-inspection'
[IO.Directory]::CreateDirectory($taskReports) | Out-Null
$taskBaseline = Get-Content "$PSScriptRoot/coexist-baseline.json" -Raw | ConvertFrom-Json
$taskOldMsi = Join-Path $taskWork 'baseline.msi'
Invoke-WebRequest -Uri $taskBaseline.url -OutFile $taskOldMsi
if ((Get-FileHash $taskOldMsi -Algorithm SHA256).Hash.ToLowerInvariant() -ne $taskBaseline.sha256) { throw 'Baseline installer checksum mismatch' }
$taskNewMsi = (Get-ChildItem "$taskRoot/packages/*.msi").FullName
$taskInstaller = New-Object -ComObject WindowsInstaller.Installer
function Get-MsiProperty([string] $path, [string] $name) {
    $database = $taskInstaller.OpenDatabase($path, 0)
    $view = $database.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property`` = '$name'")
    $view.Execute()
    return $view.Fetch().StringData(1)
}
$taskOldCode = Get-MsiProperty $taskOldMsi 'ProductCode'
$taskNewCode = Get-MsiProperty $taskNewMsi 'ProductCode'
if ($taskOldCode -eq $taskNewCode) { throw 'Product identity collision' }
if ((Get-MsiProperty $taskOldMsi 'UpgradeCode') -eq (Get-MsiProperty $taskNewMsi 'UpgradeCode')) { throw 'Upgrade identity collision' }
$taskOldFolder = Join-Path $taskWork 'baseline'
$taskNewFolder = Join-Path $taskWork 'MagicalCryptoWallet'
foreach ($item in @(@($taskOldMsi,$taskOldFolder), @($taskNewMsi,$taskNewFolder))) {
    $log = Join-Path $taskWork ((Split-Path $item[1] -Leaf)+'.log')
    $process = Start-Process msiexec.exe -WindowStyle Hidden -ArgumentList @('/i',('"'+$item[0]+'"'),'/qn','/norestart',('INSTALLFOLDER="'+$item[1]+'"'),'/L*v',('"'+$log+'"')) -PassThru -Wait
    if ($process.ExitCode -notin @(0,3010)) { throw "Installer failed: $($process.ExitCode). Inspect $log" }
}
if ($taskInstaller.ProductState($taskOldCode) -ne 5 -or $taskInstaller.ProductState($taskNewCode) -ne 5) { throw 'Both products must remain independently installed' }
if (-not (Test-Path "$taskNewFolder/magicalcryptowallet.exe") -or -not (Test-Path $taskOldFolder)) { throw 'Missing independent installation' }
$taskData = Join-Path $taskWork 'synthetic-client'
& "$taskNewFolder/magicalcryptowalletd.exe" '--help' "--datadir=$taskData" '--network=RegTest' | Out-File (Join-Path $taskWork 'help.txt')
if ($LASTEXITCODE -ne 0) { throw 'Installed daemon could not display help' }
@{ baseline_product=$taskOldCode; new_product=$taskNewCode; both_installed=$true; data_directory=$taskData } | ConvertTo-Json | Set-Content (Join-Path $taskReports 'windows-coexistence.json')
Write-Output 'Both installers coexist independently; installed daemon executes with isolated synthetic regtest storage.'
