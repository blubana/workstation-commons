# compute-mgr: keep the WSL VM below normal priority so the local Windows operator keeps ~25% CPU headroom.
Get-Process vmmemWSL -ErrorAction SilentlyContinue | Where-Object { $_.PriorityClass -ne 'BelowNormal' } | ForEach-Object { $_.PriorityClass = 'BelowNormal' }
