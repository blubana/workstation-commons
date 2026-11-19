"""cm-top v2 frames: 3 layouts x sizes, pages, states, overlays. See DESIGN.md.

Real: demo user, system/GPU/hardware/WSL values (server 2026-11-19).
INVENTED (drawn faint, noted): alice, bob, carol and their numbers.
"""
import json, sys
from pathlib import Path

SKILL = Path(__file__).resolve().parents[2] / ".claude/skills/tui-design/scripts"
sys.path.insert(0, str(SKILL))
from mockkit import Canvas, Col, spark  # noqa: E402

D = Path(__file__).parent
OUT = D / "frames"
DATA = {p.stem: json.loads(p.read_text()) for p in (D / "data").glob("*.json")}
CAPS = "attrs=bold,dim,inverse colors=16 glyphs=unicode source=assumed"
TH = "catppuccin-mocha"
F, M = "fg.faint", "fg.muted"
S = DATA["system"]
G = S["gpus"][0]
HIST = [p[1] or 0 for p in DATA["hist_cpu_percent"]["points"]] or [1, 2]
# name, mem G, cpu %, kernels, keys, gpu active, real?
USERS = [("demo", 0.0, 0.0, 0, 1, False, True), ("alice", 41.2, 812, 2, 2, True, False),
         ("bob", 18.5, 96, 1, 1, False, False), ("carol", 2.1, 3, 1, 1, False, False)]
KERNELS = [("alice-4711", "alice", "sci", "12.4G", "3h"), ("alice-4802", "alice", "sci-legacy", "28.1G", "1h"),
           ("bob-5120", "bob", "sci", "18.2G", "40m"), ("carol-6001", "carol", "sci", "2.0G", "5m")]
SHORT = ["Overview", "Users", "Kernels", "Policy", "HW", "WSL", "Envs", "Hist", "Audit"]
PAGES = ["Overview", "Users", "Kernels", "Policy", "Hardware", "WSL", "Envs", "History", "Audit"]
LAYOUT_CHIP = {"console": "F1 console", "dashboard": "F2 dashboard", "budget": "F3 budget"}


def sp(real):
    return "fg.default" if real else F


def canvas(name, state, w, h, title):
    c = Canvas(w, h, title=f"cm-top - {title}", state=state, theme=TH, caps=CAPS)
    c.note("alice, bob, carol and their numbers are INVENTED to show load; demo, system, GPU, hardware and WSL values are real")
    c.note("caps assumed (mixed terminals, ANSI-16 following the terminal theme); no test-card screenshot yet")
    return c


def header(c, y, layout, stale=False):
    left = [(" cm-top ", "accent.primary bold"), ("SERVER", "fg.default"), ("  up 1h 03m", M)]
    right = [(LAYOUT_CHIP[layout] + "  ", M), ("oomd ", M), ("✓", "status.success"), ("  admin ", M)]
    if stale:
        right = [("STALE 12s", "status.warning bold"), ("  ", M)] + right
    if c.cols < 100:
        right = right[-3:]
    c.band(y, left, right, region="header")


def footer(c, msg_y, key_y, hints, msg=("ok", "policy applied to 4 users", "2m ago")):
    if msg:
        c.message(msg_y, *msg)
    c.keybar(key_y, hints, region="keybar", overflow="drop")


def users_table(c, r, selected=1, bars=False, inactive=False, empty=False):
    if empty:
        c.lines(r.x + 1, r.y + 1, [("No compute users yet", M), ("a  add user  (name + SSH public key)", "fg.default")])
        return
    cols = [Col("USER", 8), Col("MEM", 6, "r"), Col("CPU", 5, "r"), Col("KERN", 4, "r"), Col("GPU", 3, "c")]
    if bars:
        bw = max(10, min(24, r.w - 34))
        cols.insert(2, Col("0" + "─" * (bw - 9) + "16G 80G", bw))
    else:
        cols += [Col("KEYS", 4, "r", hide_below=60), Col("LINGER", 6, hide_below=70)]
    rows = []
    for name, mem, cpu, k, keys, g, real in USERS:
        s = sp(real)
        row = [(name, s), (f"{mem:.1f}G", s), (f"{cpu:.0f}%", s), (str(k), s),
               ("●", "status.info") if g else ("·", F)]
        if bars:
            bw = cols[2].width
            n = round(mem / 80 * bw)
            g16 = round(16 / 80 * bw)
            cells = []
            for i in range(bw):
                if i < n:
                    cells.append(("█", "ramp.mid" if mem > 16 else "ramp.low"))
                elif i == g16:
                    cells.append(("┊", M))
                else:
                    cells.append(("░", F))
            row.insert(2, cells)
        else:
            row += [(str(keys), s), ("on", M)]
        rows.append(row)
    c.table(r, columns=cols, rows=rows, selected=selected, cursor=None if inactive else "❯", inactive=inactive)


def kernels_table(c, r, inactive=True):
    c.table(r, columns=[Col("ID", 10), Col("ENV", 10), Col("RSS", 6, "r"), Col("AGE", 4, "r")], inactive=inactive,
            rows=[[(k[0], F), (k[2], F), (k[3], F), (k[4], F)] for k in KERNELS])


def budget_bar(c, r, legend=True):
    w = r.w - 2
    x = r.x + 1
    for name, mem, *_rest, real in USERS:
        tok = {"demo": "ramp.low", "alice": "accent.primary", "bob": "accent.secondary", "carol": "status.info"}[name]
        n = round(mem / 104 * w)
        if n:
            c.text(x, r.y, "█" * n, tok)
            x += n
    c.text(x, r.y, "░" * (r.x + 1 + w - x), F)
    g = round(64 / 104 * w)
    c.text(r.x + 1, r.y + 1, "└" + "─" * (g - 2) + "┘", M)
    c.text(r.x + 1, r.y + 2, "guaranteed 4 x 16G = 64G of 104G", M)
    if legend:
        c.runs(r.x + 1, r.y + 3, [("demo 0.0G  ", "ramp.low"), ("alice 41.2G  ", F), ("bob 18.5G  ", F),
                                  ("carol 2.1G  ", F), ("free 42.2G", M)])


def attention(c, r, inactive=True):
    c.table(r, columns=[Col("", 1), Col("WHAT", max(10, r.w - 12)), Col("WHEN", 5, "r")], inactive=inactive, selected=0,
            rows=[[c.status("warn"), ("alice above guarantee (41.2G)", F), ("now", F)],
                  [c.status("info"), ("bob kernel idle 40m, 18.2G held", F), ("40m", F)],
                  [c.status("ok"), ("policy applied to 4 users", "fg.default"), ("2m", M)]])


def gauges(c, r, rows):
    for i, (lab, v) in enumerate(rows):
        c.text(r.x + 1, r.y + i, f"{lab:<5}", M)
        c.gauge(r.x + 7, r.y + i, r.w - 8, v, ramp=("ramp.low", "ramp.mid", "ramp.high"))


SYS_G = [("CPU", S["cpu"]["percent"] / 100), ("RAM", S["mem"]["used"] / S["mem"]["total"]),
         ("Swap", S["swap"]["used"] / S["swap"]["total"])]
GPU_G = [("Util", G["util_percent"] / 100), ("VRAM", G["mem_used"] / G["mem_total"])]


# ------------------------------------------------------------------ layouts
def console(w, h, state="normal", page=2):
    c = canvas("console", state, w, h, f"console {PAGES[page - 1]}")
    rows = [1, "*", 1, 1]
    if w < 100:
        rows = [1, 1, "*", 1, 1]
    parts = c.rect.split_v(rows)
    header(c, parts[0].y, "console", stale=state == "error")
    if w < 100:
        c.tabs(0, parts[1].y, SHORT, active=page - 1)
        main = parts[2]
    else:
        nav, main = parts[1].split_h([16, "*"])
        n = c.panel(nav, "Pages", role="sidebar")
        c.listing(n, items=[f"{i + 1} {p}" for i, p in enumerate(PAGES)], selected=page - 1, inactive=True)
    msg, keys = parts[-2], parts[-1]
    if page == 1:
        top, bot = main.split_v([7, "*"])
        b = c.panel(top, "Memory budget", role="chart", focus=False)
        budget_bar(c, b.inset(0, 1, 0, 0))
        a = c.panel(bot, "Needs attention (3)", role="list", focus=True)
        attention(c, a, inactive=False)
        hints = [("↑↓", "select"), ("enter", "open"), ("1-9", "page"), ("F1-3", "layout")]
    else:
        detail_h = 0 if (w < 100 or state == "empty") else 7
        top = main if not detail_h else main.split_v(["*", detail_h])[0]
        t = c.panel(top, f"Users ({0 if state == 'empty' else 4})", role="list", focus=True, title_right="sort mem")
        if state == "busy":
            c.table(t, columns=[Col("USER", 8), Col("MEM", 6, "r")], rows=[], skeleton=4)
        else:
            users_table(c, t, empty=state == "empty")
        if detail_h:
            d = c.panel(main.split_v(["*", detail_h])[1], "alice", role="detail")
            c.kv(d.x + 1, d.y, [("Memory", [("41.2G", F), (" of soft cap 80G  ", M), ("▲ above guarantee", "status.warning")]),
                                ("Kernels", [("2  sci, sci-legacy", F)]),
                                ("SSH keys", [("2  ed25519 alice@pc, alice@nb", F)]),
                                ("Linger", [("on (kernels survive logout)", M)])], key_w=9)
        hints = [("↑↓", "select"), ("a", "add"), ("d", "disable"), ("enter", "keys"), ("/", "filter"), ("F1-3", "layout")]
    msgs = {"error": ("error", "daemon unreachable (/run/compute-mgr/api.sock)", "r retry"),
            "busy": (None, "loading users", None), "empty": None}
    footer(c, msg.y, keys.y, hints, msgs.get(state, ("ok", "policy applied to 4 users", "2m ago")))
    return c


def dashboard(w, h, state="normal"):
    c = canvas("dashboard", state, w, h, "dashboard")
    hdr, hw, mid, msg, keys = c.rect.split_v([1, 6 if h < 30 else 7, "*", 1, 1])
    header(c, hdr.y, "dashboard", stale=state == "error")
    l, r = hw.split_h(["*", "*"], gap=1)
    a = c.panel(l, "Host - Ultra 9 285K - 24c" if w < 100 else "Host - Ultra 9 285K - 24 cores", role="panel")
    gauges(c, a, SYS_G)
    if a.h > 4:
        c.text(a.x + 1, a.y + 4, "1h ", M)
        c.text(a.x + 4, a.y + 4, spark((HIST * 30)[: a.w - 5]), "accent.primary")
    b = c.panel(r, "GPU - RTX 5080 - 16G", role="panel")
    gauges(c, b, GPU_G)
    kvs = [("Temp", f"{G['temp_c']} C   {G['power_w']:.0f} W"), ("Per-user", [("n/a under WSL", M)])]
    c.kv(b.x + 1, b.y + 2 if b.h < 5 else b.y + 3, kvs[: max(0, b.bottom - (b.y + (2 if b.h < 5 else 3)))], key_w=9)
    if w >= 100:
        ul, kl = mid.split_h([64, "*"], gap=1)
    else:
        ul, kl = mid, None
    u = c.panel(ul, "Users - mem vs 16G guarantee / 80G cap", role="list", focus=True)
    users_table(c, u, bars=True)
    if kl:
        k = c.panel(kl, "Kernels (4)", role="list")
        kernels_table(c, k)
    msg_ = ("error", "daemon unreachable (/run/compute-mgr/api.sock)", "r retry") if state == "error" else \
        ("warn", "alice above guarantee: 41.2G", "now")
    footer(c, msg.y, keys.y, [("tab", "pane"), ("s", "stop"), (":", "commands"), ("1-9", "page"), ("F1-3", "layout")], msg_)
    if state == "error":
        for row in range(mid.y, mid.bottom):
            pass
    return c


def budget(w, h, state="normal"):
    c = canvas("budget", state, w, h, "budget")
    hdr, tabs, bud, low, msg, keys = c.rect.split_v([1, 1, 7, "*", 1, 1])
    header(c, hdr.y, "budget")
    c.tabs(0, tabs.y, PAGES if w >= 100 else SHORT, active=0)
    b = c.panel(bud, "Memory budget - user.slice max 104G (WSL 110G)", role="chart", focus=True)
    budget_bar(c, b.inset(0, 1, 0, 0), legend=w >= 100)
    pl, al = low.split_h([36, "*"], gap=1)
    p = c.panel(pl, "Policy", role="form")
    c.kv(p.x + 1, p.y, [("Guarantee", "16G / user"), ("Soft cap", "80G / user"), ("Total max", "104G"),
                        ("CPU", "equal, weight 100"), ("GPU", "shared, no limit"),
                        ("oomd", [("✓ kill at 60%", "status.success")])], key_w=10)
    a = c.panel(al, "Needs attention (3)", role="list")
    attention(c, a)
    footer(c, msg.y, keys.y, [("e", "edit policy"), ("enter", "open"), ("1-9", "page"), ("F1-3", "layout")],
           (None, "policy values apply live, no reboot", None))
    return c


# ------------------------------------------------------------------ pages
def hardware(w, h):
    c = canvas("hardware", "normal", w, h, "hardware")
    hdr, body, msg, keys = c.rect.split_v([1, "*", 1, 1])
    header(c, hdr.y, "console")
    nav, main = body.split_h([16, "*"])
    n = c.panel(nav, "Pages", role="sidebar")
    c.listing(n, items=[f"{i + 1} {p}" for i, p in enumerate(PAGES)], selected=4, inactive=True)
    top, disks = main.split_v(["*", 8])
    l, r = top.split_h(["*", "*"], gap=1)
    a = c.panel(l, "Compute", role="detail", focus=True)
    c.kv(a.x + 1, a.y, [("CPU", "Intel Core Ultra 9 285K"), ("Cores", "24 (WSL may use 24)"), None,
                        ("RAM host", "128G  4 x 32G DDR5-4800"), ("RAM WSL", "110G  (user.slice max 104G)"), None,
                        ("GPU", "NVIDIA GeForce RTX 5080"), ("VRAM", "16G  driver 617.42")], key_w=9)
    b = c.panel(r, "Host", role="detail")
    c.kv(b.x + 1, b.y, [("Board", "Gigabyte Z890 UD WIFI6E"), ("OS", "Windows 11 Pro (26200)"),
                        ("Boot", "2026-11-19 20:41"), None, ("Linux", "Ubuntu 24.04 on WSL2"),
                        ("Kernel", "6.18.33.1"), ("Network", "mirrored, LAN + tailnet")], key_w=8)
    d = c.panel(disks, "Storage", role="table")
    rows = [("CT2000T500SSD8 (NVMe)", "2.0T", None), ("/   WSL ext4 vhdx", "1007G", 11 / 1007), ("C:  Windows NTFS", "1.4T", 750 / 1400)]
    for i, (name, size, used) in enumerate(rows):
        c.text(d.x + 1, d.y + i, f"{name:<24}", "fg.default")
        c.text(d.x + 26, d.y + i, f"{size:>6}", M)
        if used is not None:
            c.gauge(d.x + 34, d.y + i, d.w - 35, used, ramp=("ramp.low", "ramp.mid", "ramp.high"))
        else:
            c.text(d.x + 34, d.y + i, "physical disk", F)
    footer(c, msg.y, keys.y, [("r", "refresh"), ("1-9", "page"), ("F1-3", "layout")], (None, "Windows values cached 1h", "updated 3m ago"))
    return c


def wsl(w, h, overlay=None):
    c = canvas("wsl", "normal", w, h, "wsl")
    hdr, body, msg, keys = c.rect.split_v([1, "*", 1, 1])
    header(c, hdr.y, "console")
    nav, main = body.split_h([16, "*"])
    n = c.panel(nav, "Pages", role="sidebar")
    c.listing(n, items=[f"{i + 1} {p}" for i, p in enumerate(PAGES)], selected=5, inactive=True)
    top, cfg = main.split_v([8, "*"])
    s = c.panel(top, "WSL", role="detail", title_right="")
    c.kv(s.x + 1, s.y, [("Version", "WSL 2.7.8  kernel 6.18.33.1"), ("Windows", "10.0.26200.8655"),
                        ("Distro", [("✓ ", "status.success"), ("Ubuntu-24.04  Running  v2  default", "fg.default")]),
                        ("Autostart", [("✓ ", "status.success"), ("task WSL-Autostart  Running (at boot, no login)", "fg.default")]),
                        ("Others", [("i ", "status.info"), ("HOST\\guest also runs a WSL VM", M)])], key_w=10)
    k = c.panel(cfg, ".wslconfig  C:\\Users\\admin\\.wslconfig", role="form", focus=True)
    c.kv(k.x + 1, k.y, [("memory", "112GB"), ("processors", "24"), ("swap", "16GB"),
                        ("networking", [("mirrored", M)]), ("idle timeout", [("-1 (never stop)", M)]),
                        None, ("state", [("✓ in effect", "status.success"), ("  (no restart pending)", M)])], key_w=13)
    footer(c, msg.y, keys.y, [("e", "edit"), ("R", "restart WSL"), ("r", "refresh"), ("F1-3", "layout")], None)
    if overlay == "restart":
        inner = c.overlay(58, 10, "Restart WSL?", hints=[("enter", "restart"), ("esc", "cancel")])
        c.lines(inner.x + 1, inner.y, [("▲ Disconnects everyone and stops every kernel.", "status.warning"),
                                       ("WSL comes back by itself in about 90 s.", "fg.default"),
                                       ("Pending .wslconfig changes apply after restart.", M), "",
                                       ("Type restart to confirm:", "fg.default")])
        c.text(inner.x + 1, inner.y + 6, "restar", "fg.default bold")
        c.text(inner.x + 7, inner.y + 6, " ", "fg.default inverse")
    return c


def add_user(w, h):
    c = console(w, h, "normal", page=2)
    inner = c.overlay(64, 11, "Add user", hints=[("tab", "next field"), ("enter", "create"), ("esc", "cancel")])
    c.kv(inner.x + 1, inner.y, [("Name", [("eve", "fg.default bold"), (" ", "fg.default inverse")]),
                                ("", [("a-z 0-9 _ - , up to 31 chars", F)]), None,
                                ("SSH key", [("paste a public key (ssh-ed25519 ...)", F)]), None,
                                ("Gets", [("16G guarantee, 80G soft cap, kernels survive logout", M)])], key_w=8)
    return c


def help_overlay(w, h):
    c = console(w, h, page=1)
    inner = c.overlay(64, 20, "Keys", hints=[("esc", "close")])
    c.kv(inner.x + 1, inner.y, [("F1 F2 F3", "layout: console / dashboard / budget"), ("v", "cycle layout"),
                                ("1-9", "page (hidden pages skipped)"), ("tab", "next pane"), ("↑↓ j k", "move"),
                                ("enter", "open / drill in"), ("/", "filter table"), (":", "command palette"),
                                ("r", "refresh / retry"), ("b n", "btop / nvitop"), None,
                                ("a d x", "users: add / disable / remove key"), ("s c", "kernels: stop / Spyder connect"),
                                ("e R", "policy, WSL: edit / restart WSL")], key_w=10)
    return c


def toosmall():
    c = Canvas(70, 20, title="cm-top - too small", state="toosmall", theme=TH, caps=CAPS)
    c.toosmall((80, 24), app="cm-top")
    return c


if __name__ == "__main__":
    OUT.mkdir(exist_ok=True)
    jobs = []
    for w, h in [(80, 24), (120, 30)]:
        jobs += [(console(w, h, page=1), f"console-overview--normal--{w}x{h}"),
                 (console(w, h, page=2), f"console-users--normal--{w}x{h}"),
                 (dashboard(w, h), f"dashboard--normal--{w}x{h}"), (budget(w, h), f"budget--normal--{w}x{h}")]
    jobs += [(console(120, 30, "empty"), "console-users--empty--120x30"),
             (console(120, 30, "busy"), "console-users--busy--120x30"),
             (console(120, 30, "error"), "console-users--error--120x30"),
             (dashboard(120, 30, "error"), "dashboard--error--120x30"),
             (hardware(120, 30), "hardware--normal--120x30"), (wsl(120, 30), "wsl--normal--120x30"),
             (wsl(120, 30, "restart"), "wsl--confirm--120x30"), (add_user(120, 30), "users--addform--120x30"),
             (help_overlay(120, 30), "console--help--120x30"), (dashboard(160, 45), "dashboard--normal--160x45"),
             (toosmall(), "console--toosmall--70x20")]
    for c, name in jobs:
        c.save(str(OUT / f"{name}.mock"))
