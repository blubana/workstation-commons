# Agent runbook: set up a user's computer for the compute server

Audience: a coding agent (Claude Code, Codex, etc.) running **on the user's own computer**.
Goal: after asking at most 5 questions, leave the user able to run `ssh compute` and connect Spyder.

## Hard rules
- NEVER read, print, copy, upload or paste the private key (`id_ed25519`, no `.pub`). Only the `.pub` file may be shown.
- Never overwrite an existing key or `~/.ssh/config` entry; back up the config before editing (`config.bak-<date>`).
- Do not install system packages without asking. Do not touch the server except the test login in step 6.
- Send nothing to any service. The user delivers the public key to the admin themselves.

## Server facts
| Item | Value |
|---|---|
| LAN host | `192.168.1.10`, port `2200` |
| Remote (Tailscale) host | `<tailscale-ip>`, port `2200` |
| Auth | SSH key only (ed25519), no passwords |
| Kernel launcher on server | `spyder-kernel <env>` prints the connection-file path |
| Kernels | `spyder-kernel <spyder-version>` (3.2, 3.3, 4.0–4.2, 5.0–5.5, 6.0, 6.1); `spyder-kernel --list` |
| IDE interpreter | `/opt/conda/envs/sci/bin/python` (VS Code Remote-SSH, PyCharm SSH interpreter, Zed remote) |

## Ask the user (one message, all at once)
1. Account name the admin gave you (if none yet, use a placeholder and finish steps 1–3 now).
2. Where will you connect from: LAN only, remote via Tailscale, or both?
3. Which editor: Spyder (exact version from Help → About), VS Code, PyCharm, Zed, or none? Detect an installed Spyder version yourself if possible.
4. Is it OK to set a passphrase on the new key? (recommended: yes; the user types it, never you)

Detect the OS yourself (`uname` / `$env:OS`); don't ask.

## Steps
1. **Check for an existing key**: `~/.ssh/id_ed25519.pub` (Windows: `%USERPROFILE%\.ssh\id_ed25519.pub`). If it exists, reuse it and skip step 2.
2. **Create a key** (interactive so the user types the passphrase):
   `ssh-keygen -t ed25519 -C "<account>@compute"`. On Windows use PowerShell's built-in OpenSSH; if `ssh` is missing, tell the user to enable "OpenSSH Client" in Settings → Optional features.
3. **Show the public key** (`cat ~/.ssh/id_ed25519.pub` / `Get-Content`) and tell the user: send this one line to the admin. Pause here until the user says the account exists.
4. **Write `~/.ssh/config`** (append, after backup):
   ```
   Host compute
       HostName 192.168.1.10
       Port 2200
       User <account>
       IdentityFile ~/.ssh/id_ed25519
       IdentitiesOnly yes
   Host compute-remote
       HostName <tailscale-ip>
       Port 2200
       User <account>
       IdentityFile ~/.ssh/id_ed25519
       IdentitiesOnly yes
   ```
   Include only the hosts matching answer 2. On Windows, make sure the private key's ACL allows only the user (`icacls <key> /inheritance:r /grant:r "%USERNAME%:R"`), else ssh rejects it.
5. **Optional helper for Spyder** (if answer 3 ≠ none): create `~/compute-kernel.sh` (or `.ps1`) that runs
   `ssh compute spyder-kernel <spyder major.minor>`, parses the printed `connection file:` path, and `scp -P 2200`s it into `~/compute-kernels/`. Env from answer 3.
5b. **File transfer**: install termscp if missing (macOS `brew install termscp`, Windows `winget install termscp`, else `cargo install termscp`; ask before installing Homebrew/Rust themselves). Tell the user: `termscp sftp://<account>@<host>:2200`.
6. **Test**: `ssh -o BatchMode=yes compute 'echo ok && whoami'`. Expected `ok` and the account. On `Permission denied (publickey)`: the admin has not added the key yet, or the wrong key is used.
7. **Report to the user**: what was created (paths), the command to start a kernel, and the Spyder dialog values:
   Connect to an existing kernel → the downloaded `.json` → "remote kernel via SSH" → Hostname `192.168.1.10`, Port `2200`, Username `<account>` (Spyder 3.x: Hostname `<account>@192.168.1.10:2200`) → Keyfile `~/.ssh/id_ed25519`.

## Done when
`ssh compute` logs in without a password prompt (passphrase/agent prompt is fine) and the user knows how to start a kernel.
