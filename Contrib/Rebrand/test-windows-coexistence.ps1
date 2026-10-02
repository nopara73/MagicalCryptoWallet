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
    [void]$view.Execute()
    return $view.Fetch().StringData(1)
}
function Get-InstalledProductState([string] $productCode) {
    # PowerShell 7 requires reflection for this COM indexed property.
    return $taskInstaller.GetType().InvokeMember('ProductState', [Reflection.BindingFlags]::GetProperty, $null, $taskInstaller, @($productCode))
}
$taskOldCode = Get-MsiProperty $taskOldMsi 'ProductCode'
$taskNewCode = Get-MsiProperty $taskNewMsi 'ProductCode'
Write-Output "Installer product codes: baseline=$taskOldCode new=$taskNewCode"
if ($taskOldCode -eq $taskNewCode) { throw 'Product identity collision' }
if ((Get-MsiProperty $taskOldMsi 'UpgradeCode') -eq (Get-MsiProperty $taskNewMsi 'UpgradeCode')) { throw 'Upgrade identity collision' }
$taskOldFolder = Join-Path $taskWork 'baseline'
$taskNewFolder = Join-Path $taskWork 'MagicalCryptoWallet'
foreach ($item in @(@($taskOldMsi,$taskOldFolder), @($taskNewMsi,$taskNewFolder))) {
    $log = Join-Path $taskWork ((Split-Path $item[1] -Leaf)+'.log')
    $process = Start-Process msiexec.exe -WindowStyle Hidden -ArgumentList @('/i',('"'+$item[0]+'"'),'/qn','/norestart',('INSTALLFOLDER="'+$item[1]+'"'),'/L*v',('"'+$log+'"')) -PassThru -Wait
    Write-Output "Installer $($item[0]) exited $($process.ExitCode); target exists: $(Test-Path $item[1])"
    if ($process.ExitCode -notin @(0,3010)) { throw "Installer failed: $($process.ExitCode). Inspect $log" }
}
$taskInstaller = New-Object -ComObject WindowsInstaller.Installer
$taskOldState = Get-InstalledProductState $taskOldCode
$taskNewState = Get-InstalledProductState $taskNewCode
Write-Output "Installer registration: baseline=$taskOldState new=$taskNewState"
if ($taskOldState -ne 5 -or $taskNewState -ne 5) {
    Get-ChildItem "$taskWork/*.log" | ForEach-Object { Get-Content $_.FullName -Tail 100 }
    throw 'Both products must remain independently installed'
}
if (-not (Test-Path "$taskNewFolder/magicalcryptowallet.exe") -or -not (Test-Path $taskOldFolder)) { throw 'Missing independent installation' }
$taskData = Join-Path $taskWork 'synthetic-client'
& "$taskNewFolder/magicalcryptowalletd.exe" '--help' "--datadir=$taskData" '--network=RegTest' | Out-File (Join-Path $taskWork 'help.txt')
if ($LASTEXITCODE -ne 0) { throw 'Installed daemon could not display help' }
@{ baseline_product=$taskOldCode; new_product=$taskNewCode; both_installed=$true; data_directory=$taskData } | ConvertTo-Json | Set-Content (Join-Path $taskReports 'windows-coexistence.json')
Write-Output 'Both installers coexist independently; installed daemon executes with isolated synthetic regtest storage.'
