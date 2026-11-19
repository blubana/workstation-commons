import json
from collections import deque
from pathlib import Path

from .collect import read

AUDIT_LOG = Path("/var/log/compute-mgr/audit.jsonl")
USER_POLICY = Path("/etc/systemd/system/user-.slice.d/50-limits.conf")
TOTAL_POLICY = Path("/etc/systemd/system/user.slice.d/50-total.conf")


def policy():
    values = dict(
        line.strip().split("=", 1)
        for path in (USER_POLICY, TOTAL_POLICY)
        for line in read(path).splitlines()
        if "=" in line and not line.lstrip().startswith(("#", ";"))
    )

    def value(key, suffix=""):
        raw = values.get(key, "")
        if suffix and not raw.endswith(suffix):
            return None
        raw = raw.removesuffix(suffix)
        return int(raw) if raw.isdecimal() else None

    return {"mem_low_gb": value("MemoryLow", "G"), "mem_high_gb": value("MemoryHigh", "G"),
            "total_max_gb": value("MemoryMax", "G"), "cpu_weight": value("CPUWeight")}


def audit(limit):
    latest = deque(maxlen=limit)
    try:
        with AUDIT_LOG.open() as log:
            for line in log:
                try:
                    event = json.loads(line)
                    latest.append({key: event[key] for key in ("time", "actor", "action", "target", "ok")})
                except (ValueError, KeyError, TypeError):
                    continue
    except FileNotFoundError:
        return []
    return list(reversed(latest))
