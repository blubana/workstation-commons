# Run as admin on the Windows host: registers cm-vmmem-priority (startup + every 2 min).
New-Item -ItemType Directory -Force C:\cm-host | Out-Null
Copy-Item "$PSScriptRoot\vmmem-priority.ps1" C:\cm-host\vmmem-priority.ps1 -Force
$a = New-ScheduledTaskAction -Execute powershell.exe -Argument '-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File C:\cm-host\vmmem-priority.ps1'
$t1 = New-ScheduledTaskTrigger -AtStartup
$t2 = New-ScheduledTaskTrigger -Once -At (Get-Date) -RepetitionInterval (New-TimeSpan -Minutes 2)
$p = New-ScheduledTaskPrincipal -UserId SYSTEM -RunLevel Highest
Register-ScheduledTask cm-vmmem-priority -Action $a -Trigger $t1,$t2 -Principal $p -Description 'Reserve CPU for local operator: WSL VM BelowNormal' -Force | Out-Null
(Get-Process vmmemWSL).PriorityClass = 'Normal'
Start-ScheduledTask cm-vmmem-priority; Start-Sleep 6
"priority now: " + (Get-Process vmmemWSL).PriorityClass
