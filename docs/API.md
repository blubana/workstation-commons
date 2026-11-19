# compute-mgr API contract (v1)

Single source of truth shared by `backend/` (FastAPI) and `tui/` (ratatui).
Do not change shapes without updating this file.

## Transport
- Daemon `compute-mgrd` (FastAPI/uvicorn) runs as unprivileged system user `computemgr`.
- Listens ONLY on Unix socket `/run/compute-mgr/api.sock` (mode 0666). No TCP listener, no web UI.
- Client: TUI `cm-top`, used over SSH. Caller identity = `SO_PEERCRED` uid; no login, cookies or tokens.
- All JSON. Times are Unix epoch seconds (float). Bytes are integers. Unknown/unavailable metric => `null` (never 0).
- Errors: `{"error": "<code>", "detail": "<text>"}` with HTTP 4xx/5xx.

## Auth
- Roles: `admin` = member of Linux group `sudo` or `computeadmin`; `user` = member of group `compute`. Others: 403. No peer credentials: 401.
- `GET /api/v1/auth/me` → `{"user":"alice","uid":1001,"role":"user"}`
- Users with role `user` only see/act on their own kernels/usage; admin sees all.

## Endpoints
### GET /api/v1/system
```json
{"hostname":"SERVER","uptime_s":1234.0,
 "cpu":{"cores":2,"percent":12.5,"load":[0.1,0.2,0.3],
        "per_core":[8.0,17.0],"freq_mhz":3200.0},
 "mem":{"total":118111600640,"used":2000000000,"user_slice_max":111669149696,
        "breakdown":{"used":2000000000,"cached":1342177280,"buffers":4194304,"available":116111600640}},
 "swap":{"total":17179869184,"used":0},
 "gpus":[{"index":0,"name":"NVIDIA GeForce RTX 5080","mem_total":17095983104,"mem_used":1000000,
          "util_percent":3,"temp_c":40,"power_w":20.5,"enc_util":0,"dec_util":0,
          "clock_mhz":1200,"fan_percent":null}],
 "net":[{"iface":"eth0","rx_bps":1024.0,"tx_bps":512.0}],
 "disk_io":[{"dev":"sda","read_bps":4096.0,"write_bps":2048.0}],
 "oomd":{"active":true},
 "time":1763550000.0}
```

- `cpu.per_core` is indexed by Linux logical CPU number, with null entries for missing IDs or unavailable utilization. `cpu.freq_mhz` is the mean of the available `cpu MHz` values in `/proc/cpuinfo`, or null.
- All `mem.breakdown` fields are bytes. `used` equals `MemTotal - MemAvailable`, `cached` is `Cached`, `buffers` is `Buffers`, and `available` is `MemAvailable`. These measurements overlap.
- `net` excludes loopback. `disk_io` includes whole SCSI disk (`sd*`) and NVMe namespace (`nvme*n*`) devices. These arrays are null if the corresponding proc data is unavailable.
- Network and disk rates are bytes per second. CPU utilization and I/O rates use consecutive collector samples and are null until a valid counter delta exists.
- GPU encoder and decoder utilization and fan speed are percentages. `clock_mhz` is the NVML graphics clock. Any unsupported NVML metric is null.

### GET /api/v1/users   (admin: all compute users; user: only self)
```json
[{"user":"alice","uid":1001,"role":"user","linger":true,"keys":2,
  "usage":{"mem_current":123,"mem_low":17179869184,"mem_high":85899345920,
           "cpu_percent":150.0,"tasks":40,"gpu_mem":null,"gpu_active":true},
  "kernels":2,"sessions":1}]
```
`gpu_mem` is null when WSL cannot attribute GPU memory per process; `gpu_active` = any process of the user holds `/dev/dxg` or `/dev/nvidia*` (heuristic, may be null).

### GET /api/v1/kernels   (scoped like /users)
```json
[{"id":"alice-12345","user":"alice","pid":12345,"env":"sci-legacy","kind":"ipykernel|spyder_kernels",
  "connection_file":"/home/alice/.local/share/jupyter/runtime/kernel-sci-legacy-2267.json",
  "started":1763550000.0,"cpu_percent":0.0,"mem_rss":200000000,"gpu_active":false}]
```
### DELETE /api/v1/kernels/{id}   → 204. User may stop own; admin any. Executed via helper `kernel-stop`.

### GET /api/v1/envs → `[{"name":"sci","path":"/opt/conda/envs/sci","python":"3.12.16","spyder_kernels":"3.1.x|null"}]`

### GET /api/v1/history?metric=<m>&user=<u>&since=<epoch>&step=<s>
Metrics include `cpu_percent`, `mem_used`, `gpu_mem_used`, `gpu_util`, `user_mem`, `user_cpu`, `net_rx`, `net_tx`, `disk_read`, `disk_write`, and `gpu_temp`.
Network and disk metrics sum the available interface and device rates in bytes per second. `gpu_temp` averages available GPU temperatures in degrees Celsius.
→ `{"metric":"user_mem","user":"alice","points":[[t,v],...]}` (v may be null). Retention is 30 days in SQLite WAL, sampled every 10 s. The minimum requested step is 2 s. The server increases it as needed to return at most 2,000 buckets.

### GET /api/v1/stream   (SSE)
Events every 2 s: `event: system` (data = /system body), `event: users` (data = /users body), `event: kernels`. Each UID can open up to four streams. Additional streams receive 429.

### Admin-only (all executed through helper, see docs/HELPER.md)
- `POST /api/v1/users {"user":"bob","pubkey":"ssh-ed25519 ..."}` → helper `user-add`
- `DELETE /api/v1/users/{user}` → helper `user-disable` (locks account, kills sessions, keeps home). Helper failures are returned with an error status and detail.
- `POST /api/v1/users/{user}/keys {"pubkey":"..."}` → helper `key-add`
- `DELETE /api/v1/users/{user}/keys/{fingerprint}` → helper `key-remove`
- `GET /api/v1/users/{user}/keys` → `[{"fingerprint":"SHA256:...","type":"ssh-ed25519","comment":"bob@pc"}]`
- `GET /api/v1/policy` / `PUT /api/v1/policy {"mem_low_gb":16,"mem_high_gb":80,"total_max_gb":104,"cpu_weight":100}` → helper `policy-set`
- `GET /api/v1/audit?limit=100` → `[{"time":..,"actor":"admin","action":"user-add","target":"bob","ok":true}]`

## Hardware & WSL (v1.1)
- `GET /api/v1/hardware` (any role) → `{"cpu":{"model","logical","wsl_processors"},"memory":{"host_total","wsl_total","dimms","speed_mts","user_slice_max"},"gpus":[{"index","name","mem_total"}],"board","os","host_boot","disks":{"linux":[{"mount","total","used"}],"windows":[...],"physical":[{"name","media","size"}]}}` — any field may be null. Windows values cached 1 h.
- `GET /api/v1/wsl` (admin) → `{"wsl_version","kernel","windows","distros":[{"name","state","version","default"}],"autostart_task":"Running|Ready|missing|null","config":{"path","mtime","memory_gb","processors","swap_gb","networking_mode","vm_idle_timeout","raw":{section:{k:v}}}|null,"restart_pending":bool}` — cached 60 s.
- `PUT /api/v1/wsl/config {"memory_gb","processors","swap_gb"}` (admin) → helper `wslconfig-set`; returns `{"ok":true,...,"applies":"after WSL restart"}`.
- `POST /api/v1/wsl/restart` (admin) → 202 `{"ok":true,"restart_in_s":5,"back_within_s":90}`. Disconnects everyone, including the TUI.

## v1.2 additions
- `PUT /api/v1/users/{user}/role` (admin) body `{"role":"admin"|"user"}` → helper `user-role`; admin = member of group `computeadmin`. Takes effect on the user's next request (role is resolved per request).
- `GET /api/v1/npu/status` (any role) → `{up, npu:{name,npu_driver_version,...}, devices, openvino, cached_models, active, queued:{user:n}, served, errors, avg_latency_s}`. Non-admins receive `active: null` or `active: {user:"you"|"other"}` and `queued: {you:n, others:n}`.
- `POST /api/v1/npu/run` (any role) body `.npz` (`__xml__`, `__bin__` uint8 OpenVINO IR, optional `__device__` NPU|CPU|GPU.0, inputs by name) → `.npz` outputs + `__timing__`. One job at a time, round-robin across users, max 4 queued per user (429), 300 s queue wait (503). Uploads over `CM_NPU_MAX_BODY` (256 MiB by default) receive 413.
- Windows facts come from a forced-command SSH key (`/etc/compute-mgr/win-info`) instead of WSL interop; `.wslconfig` edits / WSL restart via a second key held by root (`win-admin`).
