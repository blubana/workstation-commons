# Privileged helper (owned by Claude; do not modify in delegated tasks)
Path: /usr/local/libexec/compute-mgr-helper (root:root 0755).
Call: `sudo -n /usr/local/libexec/compute-mgr-helper <op> '<json>'` from daemon user `computemgr` only (deploy/sudoers).
Every JSON arg object MUST include `"actor": "<authenticated linux user>"`.
**The daemon is responsible for authorization** (role checks per docs/API.md) BEFORE calling; helper re-validates arguments, ownership (kernel-stop pins pid with pidfd and checks uid + cmdline) and protected accounts.
Ops and args:
- user-add {user, pubkey}            -> {ok, user, fingerprint}
- user-disable {user}                -> {ok, user, disabled}
- key-add {user, pubkey}             -> {ok, user, fingerprint}
- key-remove {user, fingerprint}     -> {ok, user, removed}
- key-list {user}                    -> {ok, user, keys:[{fingerprint,type,comment}]}
- kernel-stop {pid:int, owner}       -> {ok, pid, stopped}
- policy-set {mem_low_gb, mem_high_gb, total_max_gb, cpu_weight} (ints) -> {ok, ...}
Errors: stdout `{"error":"validation"|"failed"|"forbidden","detail":...}`, exit 2/3.
Audit log: /var/log/compute-mgr/audit.jsonl (one JSON per line: time, actor, action, target, ok, detail).
- wslconfig-set {memory_gb 16..124, processors 2..ncpu, swap_gb 0..64} -> rewrites only memory/processors/swap in [wsl2] of the Windows .wslconfig (backup .wslconfig.bak); applies after WSL restart
- wsl-restart {}                     -> `wsl.exe --shutdown` after 5 s (kills every session); Windows task WSL-Autostart brings WSL back within ~90 s
