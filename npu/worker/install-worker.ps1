# Install cm-npu worker on the Windows host (run elevated, from the folder holding cm_npu_worker.py)
$ErrorActionPreference = 'Stop'
$D = 'C:\cm-npu'
New-Item -ItemType Directory -Force $D, "$D\cache", "$D\logs" | Out-Null
Copy-Item "$PSScriptRoot\cm_npu_worker.py" $D -Force
$py = "$env:LOCALAPPDATA\Programs\Python\Python312\python.exe"
if (-not (Test-Path "$D\venv\Scripts\python.exe")) { & $py -m venv "$D\venv" }
& "$D\venv\Scripts\python.exe" -m pip install -q --disable-pip-version-check openvino numpy
if (-not (Test-Path "$D\token")) {
  $b = New-Object byte[] 32; [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($b)
  [Convert]::ToBase64String($b) | Set-Content -NoNewline "$D\token"
}
# token readable only by SYSTEM, Administrators and this user
icacls "$D\token" /inheritance:r /grant:r "SYSTEM:R" "Administrators:R" "$($env:USERNAME):R" | Out-Null
$a = New-ScheduledTaskAction -Execute "$D\venv\Scripts\python.exe" -Argument "`"$D\cm_npu_worker.py`"" -WorkingDirectory $D
$t = New-ScheduledTaskTrigger -AtStartup
$p = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType S4U -RunLevel Highest
$s = New-ScheduledTaskSettingsSet -ExecutionTimeLimit 0 -RestartCount 99 -RestartInterval (New-TimeSpan -Minutes 1) -AllowStartIfOnBatteries
Register-ScheduledTask -TaskName 'cm-npu-worker' -Action $a -Trigger $t -Principal $p -Settings $s -Force | Out-Null
Stop-ScheduledTask cm-npu-worker -EA 0; Start-ScheduledTask cm-npu-worker
Start-Sleep 8
(Get-ScheduledTask cm-npu-worker).State
Get-Content "$D\logs\worker.log" -Tail 3
