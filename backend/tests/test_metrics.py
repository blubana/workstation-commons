import sqlite3
import sys
import time
from contextlib import closing
from types import SimpleNamespace

from compute_mgr.collect import Collector
from compute_mgr.history import RETENTION, History


def test_collectors(collector):
    system, users, kernels = collector.collect()
    assert system["mem"]["used"] == 150 * 1024
    assert system["cpu"]["load"] == [0.1, 0.2, 0.3]
    assert system["gpus"] is None
    assert users[0]["usage"]["mem_current"] == 123
    assert users[0]["usage"]["mem_high"] is None
    assert users[0]["usage"]["gpu_mem"] is None
    assert users[0]["usage"]["gpu_active"] is True
    assert kernels[0]["env"] == "sci"
    assert kernels[0]["connection_file"] == "/home/alice/kernel.json"
    assert kernels[1]["gpu_active"] is False


def test_nvml_unavailable(monkeypatch):
    class NVMLUnavailable(Exception):
        pass

    def initialize():
        raise NVMLUnavailable

    monkeypatch.setitem(sys.modules, "pynvml", SimpleNamespace(
        nvmlInit=initialize, NVMLError=NVMLUnavailable,
    ))
    assert Collector.gpu() == (None, None)


def test_history_downsampling(tmp_path):
    history = History(tmp_path / "history.db")
    history.initialize()
    now = int(time.time())
    for offset, value in [(0, 10), (10, 30), (20, None), (30, None)]:
        history.record(now + offset, [("gpu_util", "", value)])
    assert history.points("gpu_util", None, now, 20) == [[now, 20], [now + 20, None]]


def test_retention_and_wal(tmp_path):
    history = History(tmp_path / "history.db")
    history.initialize()
    now = time.time()
    history.record(now - RETENTION - 1, [("mem_used", "", 100)])
    history.record(now, [("mem_used", "", 200)])
    with closing(sqlite3.connect(history.path)) as db:
        assert db.execute("PRAGMA journal_mode").fetchone() == ("wal",)
        assert db.execute("SELECT value FROM samples").fetchall() == [(200,)]


def test_live_hardware_proc_deltas(collector, monkeypatch):
    proc = collector.proc
    now = iter((20.0, 22.0, 24.0))
    monkeypatch.setattr("compute_mgr.collect.time.monotonic", lambda: next(now))
    (proc / "cpuinfo").write_text("cpu MHz : 3200.0\ncpu MHz : 4000.0\n")
    (proc / "meminfo").write_text(
        "MemTotal: 200 kB\nMemAvailable: 50 kB\nCached: 20 kB\nBuffers: 10 kB\n"
    )
    (proc / "net").mkdir()

    def sample(active, idle, rx, tx, read, write):
        (proc / "stat").write_text(
            f"cpu {active} 0 0 {idle}\ncpu1 0 0 0 {idle}\ncpu0 {active} 0 0 80\n"
        )
        (proc / "net/dev").write_text(
            f"eth0: {rx} 0 0 0 0 0 0 0 {tx} 0 0 0 0 0 0 0\n"
            "lo: 999 0 0 0 0 0 0 0 999 0 0 0 0 0 0 0\n"
        )
        (proc / "diskstats").write_text(
            f"8 0 sda 0 0 {read} 0 0 0 {write} 0 0 0 0\n"
            f"259 0 nvme0n1 0 0 {read} 0 0 0 {write} 0 0 0 0\n"
            "8 1 sda1 0 0 0 0 0 0 0 0 0 0 0\n"
            "259 1 nvme0n1p1 0 0 0 0 0 0 0 0 0 0 0\n"
        )
        return collector.collect()[0]

    first = sample(20, 80, 512, 1024, 10, 20)
    assert first["cpu"]["per_core"] == [None, None]
    assert first["cpu"]["freq_mhz"] == 3600
    assert first["mem"]["breakdown"] == dict(
        used=150 * 1024, available=50 * 1024, cached=20 * 1024, buffers=10 * 1024,
    )
    assert first["net"] == [{"iface": "eth0", "rx_bps": None, "tx_bps": None}]
    assert first["disk_io"] == [
        {"dev": dev, "read_bps": None, "write_bps": None} for dev in ("sda", "nvme0n1")
    ]

    second = sample(30, 90, 1024, 2048, 14, 26)
    assert second["cpu"]["percent"] == 50
    assert second["cpu"]["per_core"] == [100, 0]
    assert second["net"] == [{"iface": "eth0", "rx_bps": 256, "tx_bps": 512}]
    assert second["disk_io"] == [
        {"dev": dev, "read_bps": 1024, "write_bps": 1536} for dev in ("sda", "nvme0n1")
    ]

    reset = sample(2, 91, 0, 2048, 0, 26)
    assert reset["cpu"]["per_core"] == [None, 0]
    assert reset["net"][0]["rx_bps"] is None
    assert reset["net"][0]["tx_bps"] == 0
    assert reset["disk_io"][0]["read_bps"] is None
    assert reset["disk_io"][0]["write_bps"] == 0


def test_live_hardware_history(collector, tmp_path):
    history = History(tmp_path / "history.db")
    history.initialize()
    system, users, kernels = collector.collect()
    history.sample(system, users, kernels)
    first = system["time"]
    system.update(
        time=first + 10,
        net=[{"rx_bps": 5.5, "tx_bps": None}, {"rx_bps": 2, "tx_bps": 0}],
        disk_io=[{"read_bps": 512, "write_bps": None}, {"read_bps": 1024, "write_bps": 512}],
        gpus=[{"temp_c": 30}, {"temp_c": 45}, {"temp_c": None}],
    )
    history.sample(system, users, kernels)

    for metric, expected in {
        "net_rx": 7.5, "net_tx": 0, "disk_read": 1536, "disk_write": 512, "gpu_temp": 37.5,
    }.items():
        assert history.points(metric, None, first, 10) == [[first, None], [first + 10, expected]]
