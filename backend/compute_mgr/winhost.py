"""Hardware overview and WSL status of the Windows host.

WSL interop and the C: automount are disabled on this server (any Linux user could
otherwise run Windows programs as the Windows admin). Windows facts come from one
read-only script reached over loopback SSH with a forced-command key:
  ssh -i /etc/compute-mgr/win-info $CM_WIN_TARGET   ->  JSON (C:\\cm-npu\\cm-hostinfo.ps1)
Every field degrades to None when its source is unavailable.
"""
import configparser
import json
import os
import re
import subprocess
import threading
import time

from .credentials import safe_mode

SSH_KEY = os.getenv("CM_WIN_INFO_KEY", "/etc/compute-mgr/win-info")
SSH_TARGET = os.getenv("CM_WIN_TARGET", "Administrator@127.0.0.1")
KNOWN_HOSTS = os.getenv("CM_WIN_KNOWN_HOSTS", "/etc/compute-mgr/known_hosts")
TTL = 60
_cache = {}
_lock = threading.Lock()


def _host_json():
    try:
        if not safe_mode(SSH_KEY, os.stat(SSH_KEY).st_mode):
            return None
        r = subprocess.run(["ssh", "-o", "BatchMode=yes", "-o", f"UserKnownHostsFile={KNOWN_HOSTS}",
                            "-o", "StrictHostKeyChecking=yes", "-o", "ConnectTimeout=5", "-i", SSH_KEY, SSH_TARGET],
                           capture_output=True, text=True, timeout=40)
        return json.loads(r.stdout) if r.returncode == 0 else None
    except (OSError, subprocess.TimeoutExpired, ValueError):
        return None

def _cached(key, fn, ttl=TTL):
    now = time.monotonic()
    with _lock:
        hit = _cache.get(key)
        if hit and now - hit[0] < ttl:
            return hit[1]
    value = fn()
    with _lock:
        _cache[key] = (now, value)
    return value


def invalidate():
    with _lock:
        _cache.clear()


def _cpu_model():
    try:
        with open("/proc/cpuinfo") as f:
            for line in f:
                if line.startswith("model name"):
                    return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return None


def _disk(path):
    try:
        st = os.statvfs(path)
    except OSError:
        return None
    total = st.f_blocks * st.f_frsize
    return {"mount": path, "total": total, "used": total - st.f_bfree * st.f_frsize}



def host():
    return _cached("host", _host_json)


def read_wslconfig(h=None):
    h = h if h is not None else host()
    raw = (h or {}).get("wslconfig")
    if isinstance(raw, dict):            # PowerShell 5 may serialise strings as {value: ...}
        raw = raw.get("value")
    if not isinstance(raw, str) or not raw:
        return None
    cp = configparser.ConfigParser(strict=False, interpolation=None)
    try:
        cp.read_string(raw.lstrip("\ufeff"))
    except configparser.Error:
        return None
    mtime = h.get("wslconfig_mtime")
    sec = cp["wsl2"] if cp.has_section("wsl2") else {}

    def gb(v):
        m = re.match(r"^\s*(\d+)\s*GB\s*$", v or "", re.I)
        return int(m.group(1)) if m else None

    def num(v):
        return int(v) if v and v.strip().isdigit() else None
    return {"path": h.get("wslconfig_path"), "mtime": mtime,
            "memory_gb": gb(sec.get("memory")), "processors": num(sec.get("processors")),
            "swap_gb": gb(sec.get("swap")), "networking_mode": sec.get("networkingMode"),
            "vm_idle_timeout": sec.get("vmIdleTimeout"),
            "raw": {s: dict(cp[s]) for s in cp.sections()}}



def hardware(system):
    """system = the /system snapshot (for live GPU and memory values)."""
    h = host()
    cfg = read_wslconfig(h)
    host_ = h
    gpus = [{k: g.get(k) for k in ("index", "name", "mem_total")} for g in (system.get("gpus") or [])]
    return {
        "cpu": {"model": _cpu_model(), "logical": os.cpu_count(),
                "wsl_processors": cfg and cfg["processors"]},
        "memory": {"host_total": host_ and host_.get("ram"), "wsl_total": system["mem"]["total"],
                   "dimms": host_ and host_.get("dimms"), "speed_mts": host_ and host_.get("ram_speed"),
                   "user_slice_max": system["mem"].get("user_slice_max")},
        "gpus": gpus,
        "board": host_ and " ".join(x for x in (host_.get("vendor"), host_.get("board")) if x),
        "os": host_ and f"{host_.get('os')} (build {host_.get('build')})",
        "host_boot": host_ and host_.get("boot"),
        "disks": {"linux": [d for d in (_disk("/"),) if d], "windows": [{"mount": "C:", **(host_ or {}).get("c_drive")}] if (host_ or {}).get("c_drive") else [],
                  "physical": (host_ or {}).get("disks") or []},
        "npu": (host_ or {}).get("npu"),
    }


def _parse_wsl(h):
    info = {}
    for line in (h.get("wsl_version") or "").splitlines():
        if ":" in line:
            k, v = line.split(":", 1)
            info[k.strip().lower().replace(" ", "_")] = v.strip()
    distros = []
    for line in (h.get("wsl_list") or "").splitlines()[1:]:
        parts = line.replace("*", " ").split()
        if len(parts) >= 3:
            distros.append({"name": parts[0], "state": parts[1], "version": parts[2],
                            "default": line.strip().startswith("*")})
    return {"wsl_version": info.get("wsl_version"), "kernel": info.get("kernel_version"),
            "windows": info.get("windows_version"), "distros": distros,
            "autostart_task": h.get("autostart_task") or None, "npu_task": h.get("npu_task") or None}


def wsl(boot_epoch):
    h = host() or {}
    cfg = read_wslconfig(h)
    pending = bool(cfg and cfg.get("mtime") and boot_epoch and cfg["mtime"] > boot_epoch)
    return {**_parse_wsl(h), "config": cfg, "restart_pending": pending}
