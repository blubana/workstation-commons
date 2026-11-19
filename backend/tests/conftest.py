import pytest

from compute_mgr.collect import Collector
from compute_mgr.identity import Identity


@pytest.fixture
def collector(tmp_path, monkeypatch):
    proc = tmp_path / "proc"
    proc.mkdir()
    for name, body in {
        "stat": "cpu 10 0 10 80 0 0 0 0\ncpu0 10 0 10 80\nbtime 1000\n",
        "uptime": "100.0 0",
        "loadavg": "0.1 0.2 0.3 1/5 55",
        "meminfo": "MemTotal: 200 kB\nMemAvailable: 50 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n",
    }.items():
        (proc / name).write_text(body)
    cgroup = tmp_path / "cgroup"
    for user, uid, pid in (("alice", 1001, 23), ("bob", 1002, 24)):
        directory = proc / str(pid)
        directory.mkdir()
        (directory / "status").write_text(f"Uid:\t{uid}\t{uid}\t{uid}\t{uid}\n")
        (directory / "cmdline").write_bytes(
            b"/opt/conda/envs/sci/bin/python\0-m\0ipykernel_launcher\0-f\0"
            + f"/home/{user}/kernel.json".encode() + b"\0"
        )
        (directory / "stat").write_text(f"{pid} (python worker) S " + " ".join(["0"] * 21))
        (directory / "fd").mkdir()
        (directory / "fd" / "1").symlink_to("/dev/dxg" if user == "alice" else "/dev/null")
        group = cgroup / "user.slice" / f"user-{uid}.slice"
        group.mkdir(parents=True)
        for name, body in {
            "memory.current": "123", "memory.low": "10", "memory.high": "max",
            "cpu.stat": "usage_usec 2000000", "pids.current": "3",
        }.items():
            (group / name).write_text(body)
    monkeypatch.setenv("CM_PROC_ROOT", str(proc))
    monkeypatch.setenv("CM_CGROUP_ROOT", str(cgroup))
    result = Collector()
    result.accounts = lambda: [Identity("alice", 1001, "user"), Identity("bob", 1002, "user")]
    result.gpu = lambda: (None, None)
    result.service_state = lambda: False
    return result
