import json
import math
import os
import re
import socket
import subprocess
import threading
import time
from pathlib import Path

from .identity import compute_accounts, resolve_uid

SLOW_TTL = 10


def read(path):
    try:
        return path.read_text(errors="replace").strip()
    except OSError:
        return ""


def number(value):
    try:
        return int(value)
    except (ValueError, TypeError):
        return None


def integer(path):
    return number(read(path))


def entries(path):
    try:
        return list(path.iterdir())
    except OSError:
        return []


def fields(path):
    return {
        parts[0].rstrip(":"): parts[1]
        for line in read(path).splitlines()
        if len(parts := line.split()) >= 2
    }


def difference(total, free):
    if total is not None and free is not None:
        return total - free
    return None


class Collector:
    def __init__(self, proc=None, cgroup=None):
        self.proc = Path(proc or os.getenv("CM_PROC_ROOT", "/proc"))
        self.cgroup = Path(cgroup or os.getenv("CM_CGROUP_ROOT", "/sys/fs/cgroup")) / "user.slice"
        self.accounts = compute_accounts
        self.lock = threading.Lock()
        self.previous = {}
        self.discovery_at = float("-inf")
        self.oomd_at = float("-inf")
        self.clock_ticks = os.sysconf("SC_CLK_TCK")
        self.page_size = os.sysconf("SC_PAGE_SIZE")

    @staticmethod
    def service_state():
        try:
            result = subprocess.run(
                ["systemctl", "show", "-p", "ActiveState", "--value", "systemd-oomd.service"],
                capture_output=True, text=True, timeout=1, check=True,
            )
            return result.stdout.strip() == "active"
        except (OSError, subprocess.SubprocessError):
            return None

    @staticmethod
    def gpu():
        try:
            import pynvml as nv
        except ImportError:
            return None, None

        def query(method, *args):
            try:
                return method(*args)
            except nv.NVMLError:
                return None

        try:
            nv.nvmlInit()
        except (nv.NVMLError, OSError):
            return None, None

        try:
            count = query(nv.nvmlDeviceGetCount)
            if count is None:
                return None, None
            devices, attributed = [], {}
            attribution_available = bool(count)
            for index in range(count):
                handle = query(nv.nvmlDeviceGetHandleByIndex, index)
                if handle is None:
                    attribution_available = False
                    continue
                mem = query(nv.nvmlDeviceGetMemoryInfo, handle)
                util = query(nv.nvmlDeviceGetUtilizationRates, handle)
                power = query(nv.nvmlDeviceGetPowerUsage, handle)
                name = query(nv.nvmlDeviceGetName, handle)
                encoder = query(nv.nvmlDeviceGetEncoderUtilization, handle)
                decoder = query(nv.nvmlDeviceGetDecoderUtilization, handle)
                devices.append({
                    "index": index,
                    "name": name.decode(errors="replace") if isinstance(name, bytes) else name,
                    "mem_total": mem.total if mem else None, "mem_used": mem.used if mem else None,
                    "util_percent": util.gpu if util else None,
                    "temp_c": query(nv.nvmlDeviceGetTemperature, handle, nv.NVML_TEMPERATURE_GPU),
                    "power_w": power / 1000 if power is not None else None,
                    "enc_util": encoder[0] if encoder is not None else None,
                    "dec_util": decoder[0] if decoder is not None else None,
                    "clock_mhz": query(nv.nvmlDeviceGetClockInfo, handle, nv.NVML_CLOCK_GRAPHICS),
                    "fan_percent": query(nv.nvmlDeviceGetFanSpeed, handle),
                })
                per_device = {}
                for method in (nv.nvmlDeviceGetComputeRunningProcesses, nv.nvmlDeviceGetGraphicsRunningProcesses):
                    processes = query(method, handle)
                    if processes is None:
                        attribution_available = False
                        continue
                    for process in processes:
                        used = process.usedGpuMemory
                        if used is None or used == nv.NVML_VALUE_NOT_AVAILABLE or used >= 2**64 - 1:
                            attribution_available = False
                            continue
                        per_device[process.pid] = max(used, per_device.get(process.pid, 0))
                for pid, used in per_device.items():
                    attributed[pid] = attributed.get(pid, 0) + used
            return devices or None, attributed if attribution_available else None
        finally:
            query(nv.nvmlShutdown)

    @staticmethod
    def gpu_active(process):
        try:
            descriptors = list((process / "fd").iterdir())
            unknown = False
            for fd in descriptors:
                try:
                    target = os.readlink(fd)
                except FileNotFoundError:
                    continue
                except OSError:
                    unknown = True
                    continue
                if target == "/dev/dxg" or target.startswith("/dev/nvidia"):
                    return True
            return None if unknown else False
        except OSError:
            return None

    def cpu(self, current, previous):
        usage = {}
        cores = []
        for line in read(self.proc / "stat").splitlines():
            parts = line.split()
            if not parts:
                continue
            name = parts[0]
            if core := re.fullmatch(r"cpu(\d+)", name):
                cores.append(int(core[1]))
            elif name != "cpu":
                continue
            values = list(map(number, parts[1:9]))
            if len(values) < 4 or any(value is None or value < 0 for value in values):
                continue
            total, idle = sum(values), sum(values[3:5])
            key = ("cpu", name)
            current[key] = total, idle
            if key in previous:
                last_total, last_idle = previous[key]
                if total > last_total and 0 <= idle - last_idle <= total - last_total:
                    usage[name] = 100 * (1 - (idle - last_idle) / (total - last_total))

        frequencies = []
        for line in read(self.proc / "cpuinfo").splitlines():
            name, _, value = line.partition(":")
            if name.strip() == "cpu MHz":
                try:
                    mhz = float(value)
                except ValueError:
                    continue
                if math.isfinite(mhz) and mhz > 0:
                    frequencies.append(mhz)
        return {
            "cores": len(cores) or None, "percent": usage.get("cpu"),
            "per_core": [usage.get(f"cpu{index}") for index in range(max(cores, default=-1) + 1)],
            "freq_mhz": sum(frequencies) / len(frequencies) if frequencies else None,
        }

    def io_rates(self, now, current, previous):
        def rate(key, field, scale=1):
            value = number(field)
            return self.rate(
                key, value * scale if value is not None and value >= 0 else None,
                now, current, previous, multiplier=1,
            )

        net = read(self.proc / "net/dev")
        interfaces = []
        for line in net.splitlines():
            name, colon, counters = line.rpartition(":")
            name = name.strip()
            values = counters.split()
            if colon and name and name != "lo" and len(values) >= 16:
                interfaces.append({
                    "iface": name, "rx_bps": rate(("net_rx", name), values[0]),
                    "tx_bps": rate(("net_tx", name), values[8]),
                })

        diskstats = read(self.proc / "diskstats")
        disks = []
        for line in diskstats.splitlines():
            values = line.split()
            if len(values) < 10 or not re.fullmatch(r"(?:sd[a-z]+|nvme\d+n\d+)", values[2]):
                continue
            major, minor, name = values[:3]
            disks.append({
                "dev": name, "read_bps": rate(("disk_read", major, minor, name), values[5], 512),
                "write_bps": rate(("disk_write", major, minor, name), values[9], 512),
            })
        return interfaces if net else None, disks if diskstats else None

    def system(self, timestamp, gpus, current, previous, *, now=None):
        cpu = self.cpu(current, previous)
        now = time.monotonic() if now is None else now
        net, disk_io = self.io_rates(now, current, previous)
        if now - self.oomd_at >= SLOW_TTL:
            self.oomd_active = self.service_state()
            self.oomd_at = now
        mem = fields(self.proc / "meminfo")

        def bytes_for(key):
            value = number(mem.get(key))
            return value * 1024 if value is not None else None

        try:
            load = list(map(float, read(self.proc / "loadavg").split()[:3]))
            load = load if len(load) == 3 else None
        except ValueError:
            load = None
        try:
            uptime = float(read(self.proc / "uptime").split()[0])
        except (IndexError, ValueError):
            uptime = None
        total, available = bytes_for("MemTotal"), bytes_for("MemAvailable")
        used = difference(total, available)
        return {
            "hostname": socket.gethostname(), "uptime_s": uptime,
            "cpu": {**cpu, "load": load},
            "mem": {"total": total, "used": used,
                    "user_slice_max": integer(self.cgroup / "memory.max"),
                    "breakdown": {"used": used, "cached": bytes_for("Cached"),
                                  "buffers": bytes_for("Buffers"), "available": available}},
            "swap": {"total": bytes_for("SwapTotal"),
                     "used": difference(bytes_for("SwapTotal"), bytes_for("SwapFree"))},
            "net": net, "disk_io": disk_io,
            "gpus": gpus, "oomd": {"active": self.oomd_active}, "time": timestamp,
        }

    @staticmethod
    def rate(key, value, now, current, previous, *, multiplier=100):
        if value is None:
            return None
        current[key] = value, now
        if key in previous:
            old, then = previous[key]
            if now > then and value >= old:
                return multiplier * (value - old) / (now - then)
        return None

    @staticmethod
    def process_uid(process):
        return number(fields(process / "status").get("Uid"))

    def discover(self, accounts):
        boot = number(fields(self.proc / "stat").get("btime"))
        kernels = []
        activity = {uid: [] for uid in accounts}
        for process in entries(self.proc):
            if not process.name.isdecimal():
                continue
            uid = self.process_uid(process)
            if uid is None:
                continue
            pid = int(process.name)
            active = None
            if uid in accounts:
                active = self.gpu_active(process)
                activity[uid].append(active)
            cmdline = read(process / "cmdline").strip("\0").split("\0")
            kind = next((kind for marker, kind in (
                ("ipykernel_launcher", "ipykernel"), ("spyder_kernels.console", "spyder_kernels")
            ) if any(marker in arg for arg in cmdline)), None)
            if kind is None or "-f" not in cmdline:
                continue
            index = cmdline.index("-f") + 1
            if index >= len(cmdline) or not cmdline[index] or cmdline[index].startswith("-"):
                continue
            account = accounts.get(uid)
            if account is None:
                account = resolve_uid(uid)
                if account is None:
                    continue
                active = self.gpu_active(process)
            stat = read(process / "stat").rpartition(")")[2].split()
            if len(stat) < 22:
                continue
            start = number(stat[19])
            env = re.match(r"^/opt/conda/envs/([^/]+)/", cmdline[0])
            kernels.append((process, uid, start, {
                "id": f"{account.user}-{pid}", "user": account.user, "pid": pid,
                "env": env[1] if env else None, "kind": kind, "connection_file": cmdline[index],
                "started": boot + start / self.clock_ticks if boot is not None and start is not None else None,
                "gpu_active": active,
            }))
        return activity, kernels

    def collect(self, *, fresh_discovery=False):
        with self.lock:
            return self._collect(fresh_discovery)

    def _collect(self, fresh_discovery):
        timestamp, now = time.time(), time.monotonic()
        current, previous = {}, self.previous
        gpus, attributed = self.gpu()
        system = self.system(timestamp, gpus, current, previous, now=now)
        accounts = {account.uid: account for account in self.accounts()}
        sessions = {}
        for path in entries(Path("/run/systemd/sessions")):
            for line in read(path).splitlines():
                if line.startswith("UID=") and (uid := number(line[4:])) is not None:
                    sessions[uid] = sessions.get(uid, 0) + 1
                    break
        if (fresh_discovery or now - self.discovery_at >= SLOW_TTL
                or accounts != self.discovery_accounts):
            self.activity, self.discovered_kernels = self.discover(accounts)
            self.discovery_accounts = accounts
            self.discovery_at = now

        # NVML reports live PIDs. Resolve their current owners even between discovery scans.
        gpu_memory = dict.fromkeys(accounts, 0)
        for pid, used in (attributed or {}).items():
            uid = self.process_uid(self.proc / str(pid))
            if uid in accounts:
                gpu_memory[uid] += used

        kernels = []
        for process, uid, start, kernel in self.discovered_kernels:
            stat = read(process / "stat").rpartition(")")[2].split()
            if len(stat) < 22 or number(stat[19]) != start or self.process_uid(process) != uid:
                continue
            utime, stime, rss = map(number, (stat[11], stat[12], stat[21]))
            cpu = (utime + stime) / self.clock_ticks if utime is not None and stime is not None else None
            kernels.append({
                **kernel,
                "cpu_percent": self.rate(("pid", kernel["pid"], start), cpu, now, current, previous),
                "mem_rss": rss * self.page_size if rss is not None and rss >= 0 else None,
            })
        users = []
        for uid, account in sorted(accounts.items()):
            group = self.cgroup / f"user-{uid}.slice"
            micros = number(fields(group / "cpu.stat").get("usage_usec"))
            states = self.activity[uid]
            active = True if True in states else (None if None in states else False)
            users.append({
                "user": account.user, "uid": uid, "role": account.role,
                "linger": Path("/var/lib/systemd/linger", account.user).exists(),
                "usage": {
                    "mem_current": integer(group / "memory.current"),
                    "mem_low": integer(group / "memory.low"),
                    "mem_high": integer(group / "memory.high"),
                    "cpu_percent": self.rate(
                        ("uid", uid), micros / 1_000_000 if micros is not None else None,
                        now, current, previous,
                    ),
                    "tasks": integer(group / "pids.current"),
                    "gpu_mem": gpu_memory[uid] if attributed else None,
                    "gpu_active": active,
                },
                "kernels": sum(item["user"] == account.user for item in kernels),
                "sessions": sessions.get(uid, 0),
            })
        self.previous = current
        return system, users, sorted(kernels, key=lambda item: item["pid"])

    @staticmethod
    def environments():
        result = []
        for env in sorted(entries(Path("/opt/conda/envs"))):
            if not (env / "bin/python").is_file():
                continue
            packages = {}
            for path in (env / "conda-meta").glob("*.json"):
                if not path.name.startswith(("python-", "spyder-kernels-", "spyder_kernels-")):
                    continue
                try:
                    package = json.loads(read(path))
                    packages[package["name"]] = package["version"]
                except (ValueError, KeyError):
                    continue
            spyder = packages.get("spyder-kernels") or packages.get("spyder_kernels")
            if spyder is None:
                for metadata in sorted(env.glob("lib/python*/site-packages/spyder_kernels-*.dist-info/METADATA")):
                    spyder = fields(metadata).get("Version")
                    if spyder:
                        break
            result.append({"name": env.name, "path": str(env),
                           "python": packages.get("python"), "spyder_kernels": spyder})
        return result
