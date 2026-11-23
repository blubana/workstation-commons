# Agent runbook — set up a USER's computer for the compute server

Audience: a coding agent (Claude Code, Codex, opencode, …) running **on the user's own laptop**.
Goal: ask the questions below once, then finish fully automatically. The user should end up able to run `ssh compute`, connect their Spyder or IDE, and move files.

## Hard rules
- NEVER read, print, copy, upload or paste a private key. That means any file with no `.pub` in `~/.ssh`. You may show only `.pub` files.
- Never overwrite an existing key or `~/.ssh/config` host block. Back up the config to `config.bak-<date>` before editing it.
- Ask before installing system package managers (Homebrew, Rust). Installing termscp or Tailscale through an existing manager is fine once the user agrees in Q4.
- Send nothing to any external service. The user delivers the public key to the admin.
- On the server, run only the commands in steps 7–8.

## Server facts
| Item | Value |
|---|---|
| LAN host | `192.168.1.10:2200` |
| Tailscale host | `<tailscale-ip>:2200` (port 22 is a different machine: never use it) |
| Auth | ed25519 key only |
| Kernel launcher | `spyder-kernel <spyder major.minor>` prints `connection file: <path>`. `spyder-kernel --list` |
| Supported Spyder | 3.2, 3.3, 4.0, 4.1, 4.2, 5.0–5.5, 6.0, 6.1 |
| IDE interpreter | `/opt/conda/envs/sci/bin/python` |

## Questions (ask all in one message)
1. Account name from the admin. If there is none yet, use a placeholder and stop after step 3.
2. Where will you connect from: lab LAN only, remote only, or both?
3. If you'll connect remotely: were you *invited to the lab tailnet*, or did the admin *share the machine* with you (a share link)?
4. Editor: Spyder, VS Code, PyCharm Pro, Zed, or none? May I install termscp (and Tailscale if missing)?
5. May the new key have a passphrase? Recommended: yes. The user types it, never you.

Detect these yourself and don't ask: the OS (`uname` or `$env:OS`), the installed Spyder version (`spyder --version`, or `python -c "import spyder;print(spyder.__version__)"` in its env, or the macOS app bundle's `Info.plist`), and whether `tailscale` and `termscp` are installed.

## Steps
1. **Check ssh**: run `ssh -V`. On Windows, if it is missing, tell the user to enable Settings → Optional features → OpenSSH Client.
2. **Key**: if `~/.ssh/id_ed25519.pub` exists, reuse it. Otherwise run the interactive `ssh-keygen -t ed25519 -C "<account>@<hostname>"`.
3. **Public key**: print `~/.ssh/id_ed25519.pub` and tell the user to send that one line to the admin. **Pause** until the user confirms the account exists.
4. **Tailscale** (Q2 includes remote):
   - Install Tailscale if it is missing: macOS `brew install --cask tailscale`, Windows `winget install tailscale.tailscale`, Linux `curl -fsSL https://tailscale.com/install.sh | sh` (ask first).
   - Ask the user to sign in, then accept the invite or open the share link (Q3).
   - Check that `tailscale ping -c 3 <tailscale-ip>` succeeds. If it fails, report "share not accepted, or the ACL does not allow 2200" and continue with LAN only.
5. **ssh config** (after the backup), using only the hosts Q2 needs:
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
   On Windows, run `icacls $HOME\.ssh\id_ed25519 /inheritance:r /grant:r "$($env:USERNAME):R"`.
6. **termscp** (if Q4 allows): macOS `brew install termscp`, Windows `winget install termscp`, Linux `cargo install termscp`.
7. **Test**: `ssh -o BatchMode=yes compute 'echo ok; whoami'` should print `ok` and the account name. If it fails with `Permission denied (publickey)`, the key has not been added yet or the wrong key is being used. Stop and report.
8. **Spyder helper** (Q4 = Spyder): write `~/compute-kernel.sh`, or `compute-kernel.ps1` on Windows:
   ```bash
   #!/usr/bin/env bash
   set -e; host=${1:-compute}; ver=${2:-<detected major.minor>}
   f=$(ssh "$host" spyder-kernel "$ver" | sed -n 's/^connection file: //p')
   mkdir -p ~/compute-kernels && scp "$host:$f" ~/compute-kernels/ && echo ~/compute-kernels/$(basename "$f")
   ```
   Run it once to check it works.
9. **IDE** (Q4):
   - VS Code: `code --install-extension ms-vscode-remote.remote-ssh`
   - PyCharm: give the user the SSH interpreter values.
   - Zed: `zed ssh://<account>@192.168.1.10:2200/~`

## Report to the user
List the files you created, then explain these:
- How to start a kernel: `~/compute-kernel.sh compute <ver>`.
- The Spyder dialog values. The menu is **Consoles → Connect to (an) existing kernel**:

  | Field | Value |
  |---|---|
  | Connection file | `~/compute-kernels/kernel-….json` |
  | Remote kernel via SSH | ☑ |
  | Hostname / Port / Username (Spyder 4.0–6.1) | `192.168.1.10` (remote: `<tailscale-ip>`) / `2200` / `<account>` |
  | Hostname (Spyder 3.x only) | `<account>@192.168.1.10:2200` |
  | Auth | SSH keyfile `~/.ssh/id_ed25519`, passphrase if set |

- How to verify: `socket.gethostname()` returns `SERVER`.
- How to move files: `termscp sftp://<account>@<host>:2200`.
- How to stop a kernel: `cm-top` → Kernels → `s`.

## Done when
`ssh compute` logs in without a password (a passphrase or agent prompt is fine), the helper downloads a connection file, and the user has the dialog values.
