# Run ONCE, logged in as the Windows account that should own the compute service.
# Right-click > Run with PowerShell (it self-elevates). Imports the compute WSL VM and
# registers the boot tasks under this account.
$ErrorActionPreference = 'Stop'
if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole('Administrators')) {
  Start-Process powershell.exe -Verb RunAs -ArgumentList "-NoProfile -ExecutionPolicy Bypass -NoExit -File `"$PSCommandPath`""; exit }
Start-Transcript C:\wsl\setup-as-owner.log -Force | Out-Null
$me = "$env:USERDOMAIN\$env:USERNAME"
Write-Host "Owner: $me" -ForegroundColor Cyan
wsl.exe --shutdown
if (-not ((wsl.exe -l -q) -replace "`0",'' | Where-Object { $_ -eq 'compute' })) {
  Write-Host 'Importing compute VM (C:\wsl\compute\ext4.vhdx) ...'
  wsl.exe --import-in-place compute C:\wsl\compute\ext4.vhdx
}
$p  = New-ScheduledTaskPrincipal -UserId $me -LogonType S4U -RunLevel Highest
$workerPrincipal = New-ScheduledTaskPrincipal -UserId $me -LogonType S4U -RunLevel Limited
$rs = New-ScheduledTaskSettingsSet -ExecutionTimeLimit 0 -RestartCount 99 -RestartInterval (New-TimeSpan -Minutes 1) -AllowStartIfOnBatteries
$t  = New-ScheduledTaskTrigger -AtStartup
Register-ScheduledTask cm-compute-autostart -Action (New-ScheduledTaskAction -Execute 'wsl.exe' -Argument '-d compute -u root -- sleep infinity') -Trigger $t -Principal $p -Settings $rs -Force | Out-Null
Get-ScheduledTask -TaskName 'cm-npu-worker' -ErrorAction SilentlyContinue | Stop-ScheduledTask
Register-ScheduledTask cm-npu-worker -Action (New-ScheduledTaskAction -Execute 'C:\cm-npu\venv\Scripts\python.exe' -Argument '"C:\cm-npu\cm_npu_worker.py"' -WorkingDirectory 'C:\cm-npu') -Trigger $t -Principal $workerPrincipal -Settings $rs -Force | Out-Null
Register-ScheduledTask cm-wsl-restart -Action (New-ScheduledTaskAction -Execute 'powershell.exe' -Argument '-NoProfile -Command "Start-Sleep 5; wsl.exe --shutdown"') -Principal $p -Force | Out-Null
Start-ScheduledTask cm-compute-autostart; Start-ScheduledTask cm-npu-worker
Start-Sleep 15
wsl.exe -l -v
Get-ScheduledTask cm-compute-autostart, cm-npu-worker, cm-wsl-restart | ForEach-Object { "{0,-22} {1,-8} {2}" -f $_.TaskName, $_.State, $_.Principal.UserId }
Write-Host "`nDone. You can close this window." -ForegroundColor Green
Stop-Transcript | Out-Null
