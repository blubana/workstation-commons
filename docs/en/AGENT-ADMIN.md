# Agent runbook — ADMIN tasks on the compute server

Audience: a coding agent acting **for a compute admin**, from the admin's laptop or the server itself.
Goal: onboard or offboard users, rotate keys, share Tailscale access and check health. Ask the questions below once, then do the work.

## Hard rules
- Make every account and key change through the daemon (cm-top, or the API it uses). Never edit `authorized_keys`, `/etc/passwd` or slices by hand.
- Never accept, store or transmit a private key. If a user supplies one, refuse it and tell them to make a new key.
- Get an explicit yes **in the same conversation** before any of these: WSL restart (`R`), policy change, disabling a user, stopping someone else's kernel, a Tailscale ACL edit.
- Never delete home directories or audit logs. Never open port 22 to sharees.
- Do not touch the owner account `user`; the daemon refuses with `protected account`.

## Facts
| Item | Value |
|---|---|
| Admin login | `ssh -p 2200 <admin>@192.168.1.10` (Tailscale `<tailscale-ip>`) |
| UI | `cm-top` (needs a TTY: `ssh -t …`) |
| Daemon | `compute-mgrd`, unix socket `/run/compute-mgr/api.sock` |
| Audit | `/var/log/compute-mgr/audit.jsonl` (records with `phase` `attempt` and `result`) |
| Policy | MemoryLow 24G / MemoryHigh 72G per user, 88G total, CPUWeight 100, all users ≤ 18 cores; 25% reserved for the local operator |
| Docs | `/opt/compute/{USER-GUIDE,ADMIN-GUIDE,AGENT-USER-SETUP,AGENT-ADMIN}.md` |

## Questions (ask all in one message)
1. Which task: add user, add key, remove key, disable user, share Tailscale, or health check?
2. Account name, plus the public key line (for add tasks).
3. For Tailscale: invite to the tailnet, or share the machine only? The recipient's email or the share target.
4. Confirm any disruptive action listed in the hard rules.

## Procedures
**Health check (read-only)**
```bash
ssh -p 2200 <admin>@192.168.1.10 'systemctl is-active compute-mgrd systemd-oomd; uptime; free -g; nvidia-smi --query-gpu=utilization.gpu,memory.used,memory.total --format=csv,noheader; systemctl list-units "user-*.slice" --no-legend'
```
Report the service status, the load, memory, the GPU, the active user slices, and any jobs in `system.slice` that have no limits.

**Add a user or key.** Check that the key matches `^ssh-ed25519 [A-Za-z0-9+/=]+( .*)?$`, then use cm-top: `2` → `a` (new user), or `Enter` → `a` (new key). Read the fingerprint back to the admin. Hand the user `USER-GUIDE.md` or `AGENT-USER-SETUP.md`.

**Remove a key or disable a user.** Use cm-top: `Enter` → `x`, or `d`. Then confirm the matching `result` record in the audit log shows `ok: true`.

**Tailscale share**
- The owner does this in the Tailscale admin console: Machines → `server` → Share, and sends the link.
- Agent side: check that the tailnet ACL lets sharees reach only `server:2200`. Propose the rule `{"action":"accept","src":["autogroup:shared"],"dst":["server:2200"]}` and edit only after the admin says yes.
- Afterwards, have the user run `tailscale ping <tailscale-ip>`.

**Owner batch jobs.** Launch them with `sudo cm-batch <name> -- <cmd>`, never plain `systemd-run`. This keeps them in the caller's user slice: alone they can use the whole machine, and with others they get an equal share.

## Report
List what changed: accounts, key fingerprints and share status. Include the matching audit lines, using timestamp, action, target and ok only. Never include key material beyond fingerprints.
