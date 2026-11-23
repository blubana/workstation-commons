# Compute Server — Admin Guide

For the owner (`user`) and members of the `computeadmin` group. Almost all day-to-day work happens in **cm-top**, and every admin action is written to the audit log.

## 1. Deployment at a glance

| Layer | What |
|---|---|
| Host | Windows 11 "server" → WSL2 distro `compute` (Windows account `User`) |
| Network | WSL sshd on **:2200** (LAN `192.168.1.10`, Tailscale `<tailscale-ip>`). Port 22 on the host is Windows OpenSSH (owner only). Hyper-V firewall: default **Block**, with an allow rule for LAN + `100.64.0.0/10`. |
| Daemon | `compute-mgrd` (FastAPI on unix socket `/run/compute-mgr/api.sock`, never TCP) + root sudo helper |
| UI | `cm-top` (Rust TUI). On Windows run `cm-top`; remotely run `ssh -t -p 2200 user@192.168.1.10 cm-top` |
| Limits | One systemd slice per user: MemoryLow 24G, MemoryHigh 72G, total 88G, CPUWeight 100. All users together get at most 18 cores (`user.slice.d/60-cpu-reserve.conf`). `systemd-oomd` handles memory pressure. |
| Local reserve (25%) | Memory: WSL VM 96 GB of 127.5 GB (`.wslconfig`), applied at the next WSL restart. CPU: `user.slice` quota 1800%, plus the WSL VM runs at BelowNormal priority (Windows task `cm-vmmem-priority`, every 2 min). GPU: per-process cap of 75% (`/opt/compute/lib/cm_gpu_limit.py`, override `CM_GPU_FRACTION`), and cm-top warns when VRAM is above 75%. |
| Audit | `/var/log/compute-mgr/audit.jsonl` (0640 root:computemgr). Each action writes an `attempt` record, then a `result` record. |
| User tooling | Login card (fastfetch, `/etc/xdg/fastfetch/config.jsonc`, `/etc/compute-mgr/connect.conf`), `spyder-kernel`, docs in `/opt/compute/` |

## 2. cm-top pages

Press `1`–`9` to switch pages, `?` for the keys on the current page, `/` to filter and `q` to quit.

| Key | Page | Admin actions |
|---|---|---|
| 1 | Overview | Load and warnings |
| 2 | Users | `a` add · `d` disable · `A` toggle admin · `Enter` keys (`a` add, `x` remove) |
| 3 | Kernels | All kernels · `s` stop |
| 4 | Policy | `e` edit memory/CPU policy |
| 5 | Hardware | CPU, GPU, NPU, temperatures · `l` live graphs |
| 6 | WSL | `e` edit VM memory, CPUs and swap · `R` restart WSL (**drops everyone**) |
| 7 | Envs | Shared conda envs |
| 8 | History | 30-day usage |
| 9 | Audit | Who did what, and whether it succeeded |

## 3. Users and keys

**Add a user**
1. Ask the user for their **public** key, a single line starting `ssh-ed25519 AAAA…`. If someone sends a private key, have them throw it away and generate a new one.
2. In cm-top, go to `2` Users, press `a`, enter the account name (lowercase), paste the key and press Enter.
3. Compare the fingerprint shown with the user's `ssh-keygen -lf ~/.ssh/id_ed25519.pub`.
4. Send them the account name plus `USER-GUIDE.md` (or `AGENT-USER-SETUP.md` if they use a coding agent).

Adding a user creates their home, `~/data` and their memory limits, and purges any old history recorded under the same name. The helper refuses unmanaged accounts and anyone in sudo, adm, wheel or root.

**Disable a user:** select them and press `d`, then `y`. This locks the login, kills their processes and removes their admin role. Their home directory is **kept**. To re-enable, add the user again with `a`. cm-top cannot delete accounts on purpose; the owner does that with sudo after taking a backup.

**Keys:** select a user and press `Enter`. Use one key per device; the key comment tells you which device it is. If a key is lost or leaked, remove it with `x` and add the replacement.

## 4. Tailscale sharing

The server sits in the owner's tailnet. You can give someone remote access in two ways.

| Method | When | How |
|---|---|---|
| **Invite to the tailnet** | Long-term lab members | Admin console → Users → Invite. They see the devices your ACL allows. |
| **Share the machine** | Outside people, or users who keep their own tailnet (for example the `air` Mac) | Admin console → Machines → `server` → **Share…** → copy the invite link → send it. The recipient accepts the link in their own tailnet, and they see only `server`. |

Things to check after sharing:
- The ACL must allow port **2200** to `server`. A shared node gets its ACL from the owner's tailnet, so give sharees a rule such as `{"action":"accept","src":["autogroup:shared"],"dst":["server:2200"]}`. Don't open port 22.
- The sharee should see `tailscale ping <tailscale-ip>` succeed.
- Sharing only gives network reachability. Login still needs an account and a key created in cm-top.
- Taildrop does **not** work across different owners, so send files to sharees another way.
- To revoke access, go to Machines → `server` → Share → remove the person. Disable their cm-top account as well.

## 5. Resources and policy

- **Policy** (`4` → `e`): keep MemoryLow × the number of users active at once at or below the total cap. With 24 GB × 3 = 72 GB, that stays under the 88 GB total.
- **WSL VM** (`6` → `e`): memory can be 16–124 GB. Changes apply after `R`, which restarts WSL and takes about 90 seconds. Check `3` Kernels first.
- **GPU**: there is no quota. If someone fills the VRAM, ask them first; stop their kernel with `s` only if they don't respond.
- **NPU**: the queue is automatic. If it shows *worker down*, run `Start-ScheduledTask cm-npu-worker` in an elevated PowerShell on Windows.
- **Owner / root batch jobs**: plain `systemd-run` puts jobs in `system.slice`, outside the fair share. Use `cm-batch` instead. It runs the job inside the caller's user slice, so it can use the whole machine when nobody else is computing, and gets an equal share when others are:
  ```bash
  cd /srv/myproject && sudo cm-batch train-run1 -- ./run.sh --seed 1
  systemctl status cm-batch-train-run1 ; journalctl -u cm-batch-train-run1 -f
  ```

## 6. Spyder support

`spyder-kernel <spyder-version>` selects a matching kernel env under `/opt/compute/kernels/sk-<x.y>`. `spyder-kernel --list` shows the full table. All 13 minor versions from 3.2 to 6.1 have been tested end to end through SSH tunnels. To add a new Spyder minor version: add the pinned `spyder-kernels` version to `deploy/install-spyder-kernels.sh`, rerun it as root, and add a mapping line to `deploy/spyder-kernel`.

## 7. Troubleshooting

| Symptom | Action |
|---|---|
| cm-top can't connect | `ssh -p 2200 user@192.168.1.10 'sudo systemctl restart compute-mgrd'` |
| Nobody can SSH | WSL takes 1–2 minutes after Windows boots. If it is still down, run `wsl -d compute` on Windows. |
| A user gets `Permission denied (publickey)` | Check that they used port 2200, then check that their fingerprint is in Users → Enter |
| Tailscale user times out | Check the share was accepted, that `tailscale ping` works, and that the ACL allows 2200 |
| Memory pressure | Use `8` History to find long-term heavy users, then adjust the policy |

## 8. Docs on the server (`/opt/compute/`)

`USER-GUIDE.md`, `ADMIN-GUIDE.md` (0640, computeadmin), `AGENT-USER-SETUP.md`, `AGENT-ADMIN.md` (0640, computeadmin).
