import logging
import sqlite3
import threading
import time
from contextlib import closing
from pathlib import Path

RETENTION = 30 * 24 * 60 * 60
INTERVAL = 10
MAX_BUCKETS = 2000
BYTE_METRICS = {"mem_used", "gpu_mem_used", "user_mem"}


class History:
    def __init__(self, path):
        self.path = Path(path)

    def initialize(self):
        self.path.parent.mkdir(parents=True, exist_ok=True)
        with closing(sqlite3.connect(self.path)) as db:
            db.execute("PRAGMA journal_mode=WAL")
            db.execute("""
                CREATE TABLE IF NOT EXISTS samples (
                    metric TEXT NOT NULL, user TEXT NOT NULL, time REAL NOT NULL, value REAL,
                    PRIMARY KEY (metric, user, time)
                ) WITHOUT ROWID
            """)
            db.execute("CREATE INDEX IF NOT EXISTS sample_time ON samples(time)")

    def record(self, timestamp, rows):
        with closing(sqlite3.connect(self.path)) as db, db:
            db.executemany(
                "INSERT OR REPLACE INTO samples (time, metric, user, value) VALUES (?, ?, ?, ?)",
                ((timestamp, metric, user, value) for metric, user, value in rows),
            )
            db.execute("DELETE FROM samples WHERE time < ?", (timestamp - RETENTION,))

    def purge_user(self, user):
        """Drop retained samples of a name before a new account reuses it (A1)."""
        with closing(sqlite3.connect(self.path)) as db, db:
            return db.execute("DELETE FROM samples WHERE user = ?", (user,)).rowcount

    def points(self, metric, user, since, step):
        now = time.time()
        start = max(since, now - RETENTION)
        step = max(step, (now - start) / (MAX_BUCKETS - 1))
        with closing(sqlite3.connect(self.path)) as db:
            rows = db.execute("""
                SELECT MIN(time), AVG(value) FROM samples
                WHERE metric = ? AND user = ? AND time >= ?
                GROUP BY CAST((time - ?) / ? AS INTEGER)
                ORDER BY MIN(time)
                LIMIT ?
            """, (metric, user or "", start, since, step, MAX_BUCKETS))
            return [
                [timestamp, round(value) if metric in BYTE_METRICS and value is not None else value]
                for timestamp, value in rows
            ]

    def sample(self, system, users, kernels):
        def aggregate(devices, key, combine=sum):
            values = [value for device in devices or [] if (value := device.get(key)) is not None]
            return combine(values) if values else None

        def average(values):
            return sum(values) / len(values)

        gpus = system["gpus"]
        net, disk_io = system.get("net"), system.get("disk_io")
        rows = [
            ("cpu_percent", "", system["cpu"]["percent"]),
            ("mem_used", "", system["mem"]["used"]),
            ("gpu_mem_used", "", aggregate(gpus, "mem_used")),
            ("gpu_util", "", aggregate(gpus, "util_percent", average)),
            ("gpu_temp", "", aggregate(gpus, "temp_c", average)),
            ("net_rx", "", aggregate(net, "rx_bps")),
            ("net_tx", "", aggregate(net, "tx_bps")),
            ("disk_read", "", aggregate(disk_io, "read_bps")),
            ("disk_write", "", aggregate(disk_io, "write_bps")),
        ]
        for user in users:
            rows.extend([
                ("user_mem", user["user"], user["usage"]["mem_current"]),
                ("user_cpu", user["user"], user["usage"]["cpu_percent"]),
            ])
        self.record(system["time"], rows)


class Sampler:
    def __init__(self, collect, history):
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self.run, args=(collect, history), daemon=True)

    def run(self, collect, history):
        while not self.stop.is_set():
            next_sample = time.monotonic() + INTERVAL
            try:
                history.sample(*collect())
            except Exception:
                logging.getLogger(__name__).exception("Metrics sample failed")
            self.stop.wait(max(0, next_sample - time.monotonic()))
