# Install cm-npu worker on the Windows host (run elevated, from the folder holding cm_npu_worker.py)
$ErrorActionPreference = 'Stop'
$D = 'C:\cm-npu'
Get-ScheduledTask -TaskName 'cm-npu-worker' -ErrorAction SilentlyContinue | Stop-ScheduledTask
$owner = [Security.Principal.WindowsIdentity]::GetCurrent()
$admins = [Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
$system = [Security.Principal.SecurityIdentifier]::new('S-1-5-18')
New-Item -ItemType Directory -Force $D | Out-Null
$acl = [Security.AccessControl.DirectorySecurity]::new()
$acl.SetAccessRuleProtection($true, $false)
$acl.SetOwner($admins)
$inherit = [Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit'
$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($admins, 'FullControl', $inherit, 'None', 'Allow'))
$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($system, 'FullControl', $inherit, 'None', 'Allow'))
$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($owner.User, 'ReadAndExecute', $inherit, 'None', 'Allow'))
Set-Acl -LiteralPath $D -AclObject $acl
# Reset existing descendants to the protected root ACL before installing.
if (Get-ChildItem -LiteralPath $D -Force | Select-Object -First 1) {
  icacls "$D\*" /reset /T | Out-Null
  if ($LASTEXITCODE -ne 0) { throw 'Could not reset worker ACLs' }
}
New-Item -ItemType Directory -Force "$D\cache", "$D\logs" | Out-Null
foreach ($dir in @("$D\cache", "$D\logs")) {
  $writable = Get-Acl -LiteralPath $dir
  $writable.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($owner.User, 'Modify', $inherit, 'None', 'Allow'))
  Set-Acl -LiteralPath $dir -AclObject $writable
}
Copy-Item "$PSScriptRoot\cm_npu_worker.py" $D -Force
$py = "$env:LOCALAPPDATA\Programs\Python\Python312\python.exe"
if (-not (Test-Path "$D\venv\Scripts\python.exe")) {
  & $py -m venv "$D\venv"
  if ($LASTEXITCODE -ne 0) { throw 'Could not create worker virtual environment' }
}
& "$D\venv\Scripts\python.exe" -m pip install -q --disable-pip-version-check --only-binary=:all: --require-hashes --force-reinstall -r "$PSScriptRoot\requirements.lock"
if ($LASTEXITCODE -ne 0) { throw 'Could not install locked worker requirements' }
if (-not (Test-Path "$D\token")) {
  $b = New-Object byte[] 32; [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($b)
  [Convert]::ToBase64String($b) | Set-Content -NoNewline "$D\token"
}
icacls $D /setowner '*S-1-5-32-544' /T | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Could not set worker file ownership' }
$a = New-ScheduledTaskAction -Execute "$D\venv\Scripts\python.exe" -Argument "`"$D\cm_npu_worker.py`"" -WorkingDirectory $D
$t = New-ScheduledTaskTrigger -AtStartup
$p = New-ScheduledTaskPrincipal -UserId $owner.Name -LogonType S4U -RunLevel Limited
$s = New-ScheduledTaskSettingsSet -ExecutionTimeLimit 0 -RestartCount 99 -RestartInterval (New-TimeSpan -Minutes 1) -AllowStartIfOnBatteries
Register-ScheduledTask -TaskName 'cm-npu-worker' -Action $a -Trigger $t -Principal $p -Settings $s -Force | Out-Null
Start-ScheduledTask cm-npu-worker
Start-Sleep 8
(Get-ScheduledTask cm-npu-worker).State
Get-Content "$D\logs\worker.log" -Tail 3
