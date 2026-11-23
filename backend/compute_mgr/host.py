import json
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


def reverse_lines(log):
    position = log.seek(0, 2)
    fragments = []
    while position:
        size = min(position, 64 * 1024)
        position -= size
        log.seek(position)
        first, *lines = log.read(size).split(b"\n")
        if not lines:
            fragments.append(first)
            continue
        yield b"".join([lines[-1], *reversed(fragments)])
        yield from reversed(lines[:-1])
        fragments = [first]
    if fragments:
        yield b"".join(reversed(fragments))


def audit(limit):
    latest = []
    if limit == 0:
        return latest
    try:
        with AUDIT_LOG.open("rb") as log:
            for line in reverse_lines(log):
                try:
                    event = json.loads(line)
                    if not isinstance(event, dict) or event.get("phase") == "attempt":
                        continue
                    latest.append({key: event[key] for key in ("time", "actor", "action", "target", "ok")})
                except (ValueError, KeyError, TypeError):
                    continue
                if len(latest) == limit:
                    break
    except FileNotFoundError:
        return []
    return latest
