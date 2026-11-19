# Install compute-mgr Windows-side scripts + restricted SSH keys (run elevated as the server admin)
$ErrorActionPreference = 'Stop'
$D = 'C:\cm-npu'
New-Item -ItemType Directory -Force $D | Out-Null
Copy-Item "$PSScriptRoot\cm-hostinfo.ps1","$PSScriptRoot\cm-hostadmin.ps1" $D -Force
$a = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument '-NoProfile -Command "Start-Sleep 5; wsl.exe --shutdown"'
$p = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType S4U -RunLevel Highest
Register-ScheduledTask -TaskName 'cm-wsl-restart' -Action $a -Principal $p -Force | Out-Null
$ak = 'C:\ProgramData\ssh\administrators_authorized_keys'
$keep = if (Test-Path $ak) { Get-Content $ak | Where-Object { $_ -notmatch 'compute-mgr-win-' } } else { @() }
$opts = 'no-port-forwarding,no-agent-forwarding,no-X11-forwarding,no-pty'
$new = @(
  "command=`"powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\cm-npu\cm-hostinfo.ps1`",$opts ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFKgE4ojqpBLXLm2HxnjpR0rypd/y6xbTL3HkNAKfWhy compute-mgr-win-info",
  "command=`"powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\cm-npu\cm-hostadmin.ps1`",$opts ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINGsNykrbqnqFEVt9Jg0yx3ttdSNVMxpyiCMWdr2DcMN compute-mgr-win-admin"
)
Set-Content -Path $ak -Value (@($keep) + $new) -Encoding ascii
icacls $ak /inheritance:r /grant "Administrators:F" /grant "SYSTEM:F" | Out-Null
icacls $D /inheritance:r /grant "Administrators:F" /grant "SYSTEM:F" /grant "$($env:USERNAME):F" | Out-Null
"installed"; (Get-Content $ak | Measure-Object).Count
