# cm-top — TUI design system (v2)

Built with the `tui-design` skill (`.claude/skills/tui-design`). Frames: `design/tui/frames/*.mock|png`
(generator `design/tui/frames.py`). Concepts that led here: `design/tui/concepts/`.

## 0. Brief
| Item | Value |
|---|---|
| Product shape | full-screen session over SSH (`ssh -t -p 2200 user@host cm-top`) |
| Main jobs | 1 is the server healthy / who uses what; 2 manage users + SSH keys; 3 view/edit resource policy; 4 hardware overview; 5 WSL status + config + restart; 6 kernels (stop, Spyder connect help) |
| Users | admin (group sudo/computeadmin) sees all; user (group compute) sees only self, no admin pages |
| Data | API `docs/API.md` over `/run/compute-mgr/api.sock` (SSE `/stream` 2 s, history, hardware, wsl) |
| Sizes | min 80×24 · common 120×30 · wide ≥160×45. Below 80×24 → too-small screen |
| Capability profile | `attrs=bold,dim,inverse colors=16 glyphs=unicode source=assumed` — mixed Windows Terminal / macOS / Linux terminals; **no italic, no strike, no truecolor, no Nerd Font, no emoji/CJK in UI text** |
| Theme | follow the terminal: ANSI 16 slots only (`Color::Reset`, `Color::Black..White`, `LightX`), never RGB/indexed-256. Tokens map to ANSI slots via the theme's `ansi16`. Respect `NO_COLOR` (attributes only). |
| Language | en-US |
| Must keep | all functions of the current cm-top (7 tabs, keys view, Spyder popup, mock mode, role gating) |
| Future | Tailscale access: host names in help/connect text come from config (`/etc/compute-mgr/tui.toml` `[connect] hosts = ["192.168.1.10", "server.<tailnet>.ts.net"]`), never hard-coded |

## 1. Layouts (switchable "concepts")
Three layouts over the same data and pages. `F1`/`F2`/`F3` pick, `v` cycles. Choice persists per user in
`~/.config/cm-top/state.toml` (`layout = "console"`); default `console`.

| Layout | Shape | Exemplar | What it optimises |
|---|---|---|---|
| **Console** (F1) | left sidebar of pages + main pane (table) + detail pane | k9s / lazydocker | management: every page full-size |
| **Dashboard** (F2) | one screen: host + GPU gauges, users with memory-vs-policy bars, kernels; pages open as full-screen overlays | btop | "is everything healthy right now?" |
| **Budget** (F3) | top tabs; memory budget bar split by user with the guarantee bracket; policy card + "needs attention" list | (signature) | dynamic allocation at a glance |

Pages (all layouts): `1 Overview · 2 Users · 3 Kernels · 4 Policy · 5 Hardware · 6 WSL · 7 Envs · 8 History · 9 Audit`.
Non-admin: only `1 Overview · 3 Kernels · 5 Hardware · 7 Envs · 8 History` (numbers keep their meaning; hidden ones are skipped).
- Console: pages = sidebar entries; Overview page = the Budget bar + attention list (merged concept).
- Dashboard: the dashboard *is* Overview; number keys open that page full-screen, `esc` returns.
- Budget: pages = tabs; Overview tab = budget screen.

## 2. Pages (content is layout-independent)
| Page | Content | Actions (admin unless noted) |
|---|---|---|
| Overview | budget bar (each user's current mem, guarantee bracket, free), CPU share line, attention list (above guarantee, near soft cap, idle kernel > 30 min holding > 4G, oomd kill, restart pending, daemon degraded) | `enter` jump to the item's page |
| Users | table USER ROLE MEM CPU KERN GPU KEYS LINGER; detail: mem vs low/high, kernels, keys | `a` add user (form: name + pubkey paste) · `d` disable (type name to confirm) · `A` toggle admin role (y/N) · `enter` keys view (`a` add, `x` remove) |
| Kernels | ID USER ENV RSS CPU AGE GPU | `s` stop (confirm; users: own only) · `c` Spyder connect popup (every configured host) |
| Policy | four values + live effect preview (guarantee × users vs total) | `e` edit form, validation `low ≤ high ≤ total`, preview before apply |
| Hardware | CPU model/cores (WSL limit), RAM host vs WSL (DIMMs, speed), GPU(s), board, Windows OS, disks (physical + / + C:) with usage bars | read-only |
| WSL | version/kernel, distros + state, autostart task state, `.wslconfig` (memory, processors, swap, networking, idle timeout), `restart pending` chip | `e` edit memory/processors/swap · `R` restart WSL (type `restart`; warns: disconnects everyone, back in ~90 s) |
| Envs | NAME PYTHON SPYDER-KERNELS PATH | read-only |
| History | metric picker + user picker + range 1h/6h/24h/7d, chart with null gaps | `m` metric · `u` user · `r` range |
| Audit | TIME ACTOR ACTION TARGET RESULT, newest first, ✓/✗ | `/` filter |

## 3. States (every page)
- `normal`; `empty` (says why + the key that fills it, e.g. "No users yet · a add user");
- `busy` (first load: skeleton rows; action in flight: spinner in the message line, rest usable);
- `error` = daemon unreachable / API error: message line `✗ daemon unreachable (/run/compute-mgr/api.sock) · r retry`, last data kept and drawn **stale (faint) with a `STALE 12s` chip** in the header;
- `toosmall` (< 80×24): "cm-top needs 80×24, window is 70×20 · q quit".
- WSL restart in flight: full-screen notice "WSL restarting · reconnect in ~90 s"; the TUI exits 0 after 3 s.

## 4. Keymap
Global (footer right): `? help · q quit`. Footer left = context hints for the focused pane, max ~6.
| Key | Action | Scope |
|---|---|---|
| `F1` `F2` `F3` / `v` | layout console / dashboard / budget / cycle | global |
| `1`–`9` | page | global (skips hidden) |
| `tab` / `shift-tab` | next / previous pane | layouts with panes |
| `↑↓` `j k`, `PgUp PgDn`, `g G` | move / page / top / bottom | lists |
| `enter` | open / drill in | lists |
| `esc` | close overlay / back | overlays, dashboard page view |
| `/` | filter current table (fzf-like, `esc` clears) | tables |
| `r` | refresh now / retry | global |
| `:` | command palette (every action by name, fuzzy) | global; doubles as discoverability |
| `b` `n` | suspend → btop / nvitop | global |
| page actions | see §2 (`a d A x e s c R m u`) | page |
Confirmations: destructive ones (disable user, remove key, stop kernel, WSL restart) need `y` (stop kernel, remove key) or typing the name (`disable`, `restart`). Role changes require `y`. Default focus on **Cancel**.

## 5. Visual rules
- Status = glyph + color: `✓` ok (green) · `▲` warning (yellow) · `✗` error (red) · `i` info (cyan) · `●` active (cyan) · `·` none (faint).
- Bars: `█` fill, `░` track (faint); memory bars mark the guarantee with `┊` and the soft cap with `│`.
- Selection: inverse band (`selection.bg` → ANSI reverse) + `❯` cursor in the focused pane; unfocused panes keep selection without cursor, dim.
- Selected row keeps bar and status colours: draw bar cells with their own fg on the selection bg (never reverse a bar). Mockups show this wrong; the rule wins.
- Borders: rounded; focused pane border `border.focus` (blue/bright), others `border.default` (faint).
- Numbers right-aligned; sizes `41.2G`; time relative (`3h`, `40m`); null → `n/a` faint.
- Header: `cm-top host · up · STALE chip? · oomd ✓ · role user`; message line above footer; footer key hints.
- 16-colour floor: everything must read at `--depth 16` and with `NO_COLOR`.

## 6. Too-small / narrow behavior
- 80×24: Console sidebar collapses to a 1-row page strip at top (like Budget tabs); detail pane hidden (`enter` opens it as overlay). Dashboard stacks gauges (2 rows) + users; kernels become a count line. Budget: attention list under policy.
- 60-column floor: below 80 cols the too-small screen shows (minimum width 80).

## 7. Open decisions
1. Attention thresholds (idle 30 min / 4G) are guesses — confirm.
2. Whether non-admins may see other users' *names* on the budget bar (now: own segment + "others" aggregated).
3. Command palette scope: actions only, or also jump-to-user/kernel.
