# Read-only host facts for compute-mgr (forced command of the compute-mgr "info" SSH key).
$ErrorActionPreference = 'SilentlyContinue'
$c = Get-CimInstance Win32_ComputerSystem; $o = Get-CimInstance Win32_OperatingSystem
$m = Get-CimInstance Win32_PhysicalMemory
$ver = (& wsl.exe --version) -join "`n" -replace "`0", ''
$list = (& wsl.exe -l -v) -join "`n" -replace "`0", ''
$task = (Get-ScheduledTask cm-compute-autostart, WSL-Autostart -EA SilentlyContinue | Select-Object -First 1).State
$npuTask = (Get-ScheduledTask cm-npu-worker).State
$cfgPath = "$env:USERPROFILE\.wslconfig"
$cfg = if (Test-Path $cfgPath) { [string](Get-Content $cfgPath -Raw) } else { $null }
$cfgMtime = if (Test-Path $cfgPath) { [int64]((Get-Item $cfgPath).LastWriteTimeUtc - [datetime]'1970-01-01').TotalSeconds } else { $null }
$vol = Get-Volume -DriveLetter C
$npu = Get-PnpDevice -PresentOnly | Where-Object { $_.Class -eq 'ComputeAccelerator' } | Select-Object -First 1
$npuDrv = if ($npu) { (Get-CimInstance Win32_PnPSignedDriver | Where-Object { $_.DeviceName -eq $npu.FriendlyName } | Select-Object -First 1).DriverVersion }
[pscustomobject]@{
  board = $c.Model; vendor = $c.Manufacturer; os = $o.Caption; build = $o.BuildNumber
  ram = $c.TotalPhysicalMemory; dimms = @($m).Count; ram_speed = ($m | Select-Object -First 1).Speed
  boot = $o.LastBootUpTime.ToUniversalTime().ToString('o')
  disks = @(Get-PhysicalDisk | ForEach-Object { [pscustomobject]@{ name = $_.FriendlyName; media = [string]$_.MediaType; size = $_.Size } })
  c_drive = [pscustomobject]@{ total = $vol.Size; used = $vol.Size - $vol.SizeRemaining }
  wsl_version = $ver; wsl_list = $list; autostart_task = [string]$task; npu_task = [string]$npuTask
  wslconfig = $cfg; wslconfig_path = $cfgPath; wslconfig_mtime = $cfgMtime
  npu = if ($npu) { [pscustomobject]@{ name = $npu.FriendlyName; status = $npu.Status; driver = $npuDrv } } else { $null }
} | ConvertTo-Json -Depth 4 -Compress
