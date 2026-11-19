$ErrorActionPreference='Stop'
Copy-Item "$HOME\wslconfig.txt" "$HOME\.wslconfig" -Force
# Hyper-V firewall: block inbound by default and merge Windows firewall policy.
Set-NetFirewallHyperVVMSetting -Name '{40E0AC32-46A5-438A-A0B2-2B479E8F2E90}' -DefaultInboundAction Block -AllowHostPolicyMerge True
# Windows firewall: SSH from the local subnet and Tailscale.
Get-NetFirewallRule -DisplayName 'WSL-SSH-2222','WSL-SSH-2200' -EA 0 | Remove-NetFirewallRule
New-NetFirewallRule -DisplayName 'WSL-SSH-2200' -Direction Inbound -Protocol TCP -LocalPort 2200 -RemoteAddress LocalSubnet,100.64.0.0/10 -Action Allow | Out-Null
# Autostart WSL at boot without login
$a = New-ScheduledTaskAction -Execute 'wsl.exe' -Argument '-d Ubuntu-24.04 -u root -- /bin/sh -c "exec sleep infinity"'
$t = New-ScheduledTaskTrigger -AtStartup
$p = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType S4U -RunLevel Highest
$s = New-ScheduledTaskSettingsSet -ExecutionTimeLimit 0 -RestartCount 99 -RestartInterval (New-TimeSpan -Minutes 1) -AllowStartIfOnBatteries
Register-ScheduledTask -TaskName 'WSL-Autostart' -Action $a -Trigger $t -Principal $p -Settings $s -Force | Out-Null
# Windows Update: active hours 08-23
Set-ItemProperty 'HKLM:\SOFTWARE\Microsoft\WindowsUpdate\UX\Settings' -Name ActiveHoursStart -Value 8 -Type DWord
Set-ItemProperty 'HKLM:\SOFTWARE\Microsoft\WindowsUpdate\UX\Settings' -Name ActiveHoursEnd -Value 23 -Type DWord
wsl.exe --shutdown
Start-ScheduledTask -TaskName 'WSL-Autostart'
"done"
