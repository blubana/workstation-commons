# cm-top

Terminal management for compute-mgr. The daemon identifies the caller by Unix socket peer UID. The default socket is `/run/compute-mgr/api.sock`.

## Build and run

```sh
cd tui
cargo test
cargo build --release --locked
./target/release/cm-top
CM_SOCK=/path/to/api.sock ./target/release/cm-top
```

Run the executable on the compute host, for example in an SSH session. The optional btop and nvitop shortcuts require those programs on `PATH`.

## Controls

| Where | Key | Action |
| --- | --- | --- |
| Global | `1`–`7` | Open the numbered tab shown in the header |
| Global | `Tab` / `Shift+Tab` | Cycle forward or back through visible tabs |
| Tables | `↑` / `↓` or `k` / `j` | Select the previous or next row and scroll it into view |
| Global | `r` | Refresh the current view |
| Global | `?` | Show the current tab’s help |
| Global | `b` / `n` | Run btop or nvitop and return on exit |
| Global | `q` | Quit |
| `1` Overview | `↑` / `↓` or `k` / `j` | Browse users and view system gauges, kernels, and ten-minute CPU/RAM sparklines |
| `2` Kernels | `s` | Confirm stopping the selected kernel |
| `2` Kernels | `c` | Display the selected kernel’s connection path, SCP command, and Spyder SSH settings |
| `3` Users | `a` | Add a user with a username and SSH public key |
| `3` Users | `d` | Disable the selected user by typing the username |
| `3` Users | `Enter` | Open the selected user’s SSH keys |
| SSH keys | `a` / `x` | Add a public key or confirm removing the selected key |
| SSH keys | `Esc` | Return to the users table |
| `4` Policy | `e` | Edit the four integer policy fields |
| `5` History | `m` | Cycle through CPU, memory, GPU memory, GPU utilization, user memory, and user CPU |
| `5` History | `u` | Cycle through users for a user metric as an admin |
| `5` History | `t` | Cycle through 1h, 6h, 24h, and 7d |
| `6` Envs | Table navigation | Browse Python environments |
| `7` Audit | Table navigation | Browse up to 200 entries, with successes in green and failures in red |
| Forms | `Tab` / `Shift+Tab` or `↓` / `↑` | Focus the next or previous field |
| Forms | Type or paste | Insert text in the field using the terminal’s paste shortcut |
| Forms | `←` / `→`, `Home` / `End`, `Backspace` / `Delete` | Move the cursor or edit text |
| Forms | `Enter` / `Esc` | Submit or cancel |
| Confirmations | `y` / `n` or `Esc` | Confirm or cancel |
| Help and Spyder popups | `↑` / `↓` or `k` / `j`, `Home` | Scroll or return to the top |
| Help and Spyder popups | `Enter`, `Esc` or `?` | Close the popup |

Users, Policy, and Audit are admin-only. Regular users see their own kernels and usage. The Overview uses Unix socket events for live metrics and falls back to polling. History retains gaps for null samples and displays memory in binary units and utilization in percent.

## Mock mode

```sh
./target/release/cm-top --mock fixtures
```

The fixtures directory contains snapshots, environment and audit rows, policy, a username-to-key-list map in `users-keys.json`, and history for all six metrics. User history fixtures map usernames to histories. Mock mutations update the running session’s users, kernels, keys, policy, and audit as appropriate. Restarting reloads the fixtures from disk.

### Dependency security

The current lockfile resolves Ratatui 0.29 with `lru` 0.12.5 and `paste` 1.0.15.
Ratatui 0.30.2 is an upgrade candidate with a newer `lru` and no `paste` in its
published lockfile. The upgrade could not be resolved in the offline build
environment because the Cargo cache lacks `ratatui-core`.
