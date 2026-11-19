# Whitelisted admin actions for compute-mgr (forced command of the helper's "admin" SSH key).
# Usage (SSH_ORIGINAL_COMMAND):  wslconfig-set <memory_gb> <processors> <swap_gb>  |  wsl-restart
$ErrorActionPreference = 'Stop'
$argv = ($env:SSH_ORIGINAL_COMMAND -split '\s+') | Where-Object { $_ }
function Fail($m) { [pscustomobject]@{ error = 'validation'; detail = $m } | ConvertTo-Json -Compress; exit 2 }
switch ($argv[0]) {
  'wslconfig-set' {
    if ($argv.Count -ne 4) { Fail 'need memory_gb processors swap_gb' }
    $n = @(); foreach ($a in $argv[1..3]) { if ($a -notmatch '^\d{1,3}$') { Fail "not an integer: $a" }; $n += [int]$a }
    $cores = (Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors
    if ($n[0] -lt 16 -or $n[0] -gt 124) { Fail 'memory_gb must be 16..124' }
    if ($n[1] -lt 2 -or $n[1] -gt $cores) { Fail "processors must be 2..$cores" }
    if ($n[2] -lt 0 -or $n[2] -gt 64) { Fail 'swap_gb must be 0..64' }
    $p = "$env:USERPROFILE\.wslconfig"
    $lines = if (Test-Path $p) { Get-Content $p } else { @() }
    Copy-Item $p "$p.bak" -Force -EA SilentlyContinue
    $want = [ordered]@{ memory = "$($n[0])GB"; processors = "$($n[1])"; swap = "$($n[2])GB" }
    $out = New-Object System.Collections.Generic.List[string]; $sec = ''; $seen = @{}
    $hasWsl2 = $false
    foreach ($l in $lines) {
      if ($l -match '^\s*\[(.+?)\]\s*$') {
        if ($sec -eq 'wsl2') { foreach ($k in $want.Keys) { if (-not $seen[$k]) { $out.Add("$k=$($want[$k])") } } }
        $sec = $Matches[1].Trim().ToLower(); if ($sec -eq 'wsl2') { $hasWsl2 = $true }
      } elseif ($sec -eq 'wsl2' -and $l -match '^\s*([A-Za-z]+)\s*=' -and $want.Contains($Matches[1].ToLower())) {
        $k = $Matches[1].ToLower(); $seen[$k] = $true; $l = "$k=$($want[$k])"
      }
      $out.Add($l)
    }
    if ($sec -eq 'wsl2') { foreach ($k in $want.Keys) { if (-not $seen[$k]) { $out.Add("$k=$($want[$k])") } } }
    if (-not $hasWsl2) { $out.Insert(0, '[wsl2]'); $i = 1; foreach ($k in $want.Keys) { $out.Insert($i, "$k=$($want[$k])"); $i++ } }
    Set-Content -Path $p -Value $out -Encoding ascii
    [pscustomobject]@{ ok = $true; memory_gb = $n[0]; processors = $n[1]; swap_gb = $n[2]; applies = 'after WSL restart' } | ConvertTo-Json -Compress
  }
  'wsl-restart' {
    Start-ScheduledTask -TaskName 'cm-wsl-restart'
    [pscustomobject]@{ ok = $true; restart_in_s = 5; back_within_s = 90 } | ConvertTo-Json -Compress
  }
  default { Fail 'unknown action' }
}
