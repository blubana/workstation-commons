# compute-mgr backend

From this directory, install with Python 3.12 and run the tests.

```sh
uv sync
uv run pytest -q
uv run python -c "import compute_mgr.app"
```

For a local run, set `CM_DB` and `CM_SOCK` to writable paths and start the daemon with `uv run compute-mgrd`. It listens only on the Unix socket; there is no TCP listener.

| Variable | Default |
| --- | --- |
| `CM_PROC_ROOT` | `/proc` |
| `CM_CGROUP_ROOT` | `/sys/fs/cgroup` |
| `CM_DB` | `/var/lib/compute-mgr/metrics.db` |
| `CM_SOCK` | `/run/compute-mgr/api.sock` |
| `CM_HELPER` | `/usr/local/libexec/compute-mgr-helper` |
| `CM_PUBLIC_HOST` | Local hostname |



History averages samples in buckets anchored to `since` and returns the first actual sample timestamp in each occupied bucket. Nulls are skipped when computing the average. Empty buckets are omitted and an all-null bucket returns null. The default step is 10 seconds and the default history window is 30 days. A user-history query without a username defaults to the caller. Policy reads use the systemd drop-ins written by the helper.
