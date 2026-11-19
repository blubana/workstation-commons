"""Three genuinely different concepts for cm-top (120x30, normal state).

Real data: design/tui/data/*.json (captured from the server 2026-11-19) and the
measured hardware/WSL facts. Users other than `demo` are INVENTED (drawn faint).
"""
import json, os, sys
from pathlib import Path

SKILL = Path(__file__).resolve().parents[2] / ".claude/skills/tui-design/scripts"
sys.path.insert(0, str(SKILL))
from mockkit import Canvas, Col, spark  # noqa: E402

D = Path(__file__).parent
OUT = D / "concepts"
DATA = {p.stem: json.loads(p.read_text()) for p in (D / "data").glob("*.json")}
CAPS = "attrs=bold,dim,inverse colors=16 glyphs=unicode source=assumed"
THEME = "catppuccin-mocha"
GB = 1 << 30
sysd = DATA["system"]
gpu = sysd["gpus"][0]
cpu_hist = [p[1] or 0 for p in DATA["hist_cpu_percent"]["points"]] or [1, 2, 1]
# demo is real; the rest invented to show a 4-user load
USERS = [("demo", 0.006, 0.0, 0, 1, False),
         ("alice", 41.2, 812.0, 2, 2, True), ("bob", 18.5, 96.0, 1, 1, False),
         ("carol", 2.1, 3.0, 1, 1, False)]
F = "fg.faint"


def head(c, y, w, right_extra=()):
    c.band(y, [(" cm-top ", "accent.primary bold"), ("SERVER", "fg.default"),
               ("  up 1h 03m", "fg.muted")],
           [("oomd ", "fg.muted"), ("✓ on", "status.success"), ("  admin admin ", "fg.muted"), *right_extra],
           region="header")


def note_invented(c):
    c.note("users alice, bob, carol and their numbers are invented to show load; demo + system values are real")
    c.note("caps assumed: mixed Windows/Mac/Linux terminals, 16-colour ANSI following the terminal theme")


# ---------------------------------------------------------------- concept A
def concept_a():
    """Console: left nav of resources + main table + detail (k9s / lazydocker)."""
    c = Canvas(120, 30, title="cm-top — console (Users)", state="normal", theme=THEME, caps=CAPS)
    hdr, body, msg, keys = c.rect.split_v([1, "*", 1, 1])
    head(c, hdr.y, 120)
    nav, main = body.split_h([20, "*"], gap=0)
    n = c.panel(nav, "Views", role="sidebar")
    c.listing(n, items=["1 Overview", "2 Users", "3 Kernels", "4 Policy", "5 Hardware", "6 WSL", "7 Envs", "8 Audit"],
              selected=1, inactive=True)
    top, det = main.split_v(["*", 9])
    t = c.panel(top, "Users (4)", role="list", focus=True, title_right="sort: mem")
    rows = []
    for u, mem, cpu, k, keys_n, g in USERS:
        sp = "fg.default" if u == "demo" else F
        rows.append([(u, sp), (f"{mem:5.1f}G", sp), (f"{cpu:5.0f}%", sp), (str(k), sp),
                     (("●" if g else "·"), "status.info" if g else F), (str(keys_n), sp), ("16G / 80G", "fg.muted")])
    c.table(t, columns=[Col("USER", 12), Col("MEM", 7, "r"), Col("CPU", 6, "r"), Col("KERN", 4, "r"),
                        Col("GPU", 3, "c"), Col("KEYS", 4, "r"), Col("GUARANTEE / SOFT CAP", 20)],
            rows=rows, selected=1, cursor="❯")
    d = c.panel(det, "alice", role="detail")
    c.kv(d.x + 1, d.y, [("Memory", [("41.2G", F), ("  of soft cap 80G  ", "fg.muted"), ("▲ above guarantee", "status.warning")]),
                        ("Kernels", [("2  sci, sci-legacy", F)]),
                        ("SSH keys", [("2  ed25519 alice@pc, ed25519 alice@nb", F)]),
                        ("Linger", [("on (kernels survive logout)", "fg.muted")])], key_w=10)
    c.message(msg.y, "ok", "policy applied to 4 users", "2m ago")
    c.keybar(keys.y, [("↑↓", "select"), ("a", "add user"), ("d", "disable"), ("enter", "keys"), ("1-8", "view")],
             region="keybar")
    note_invented(c)
    c.save(str(OUT / "console--normal--120x30.mock"))


# ---------------------------------------------------------------- concept B
def concept_b():
    """Dashboard: everything at a glance (btop), actions via palette."""
    c = Canvas(120, 30, title="cm-top — dashboard", state="normal", theme=THEME, caps=CAPS)
    hdr, hw, mid, msg, keys = c.rect.split_v([1, 8, "*", 1, 1])
    head(c, hdr.y, 120)
    l, r = hw.split_h(["*", "*"], gap=1)
    a = c.panel(l, "Host · Ultra 9 285K · 24 cores", role="panel")
    for i, (lab, v) in enumerate([("CPU", sysd["cpu"]["percent"] / 100), ("RAM", sysd["mem"]["used"] / sysd["mem"]["total"]),
                                  ("Swap", sysd["swap"]["used"] / sysd["swap"]["total"])]):
        c.text(a.x + 1, a.y + i, f"{lab:<5}", "fg.muted")
        c.gauge(a.x + 7, a.y + i, a.w - 9, v, ramp=("ramp.low", "ramp.mid", "ramp.high"))
    c.text(a.x + 1, a.y + 4, "CPU 1h ", "fg.muted")
    c.text(a.x + 8, a.y + 4, spark((cpu_hist * 20)[: a.w - 10]), "accent.primary")
    b = c.panel(r, f"GPU · {gpu['name'].replace('NVIDIA GeForce ', '')} · 16G", role="panel")
    for i, (lab, v) in enumerate([("Util", gpu["util_percent"] / 100), ("VRAM", gpu["mem_used"] / gpu["mem_total"])]):
        c.text(b.x + 1, b.y + i, f"{lab:<5}", "fg.muted")
        c.gauge(b.x + 7, b.y + i, b.w - 9, v, ramp=("ramp.low", "ramp.mid", "ramp.high"))
    c.kv(b.x + 1, b.y + 3, [("Temp", f"{gpu['temp_c']} C"), ("Power", f"{gpu['power_w']:.0f} W"),
                            ("Per-user", [("n/a under WSL", "fg.muted")])], key_w=9)
    ul, kl = mid.split_h([62, "*"], gap=1)
    u = c.panel(ul, "Users · memory vs guarantee 16G / soft cap 80G", role="list", focus=True)
    rows = []
    for name, mem, cpu, k, _, g in USERS:
        sp = "fg.default" if name == "demo" else F
        rows.append([(name, sp), (f"{mem:5.1f}G", sp), [("█" * int(mem / 80 * 20), "ramp.mid" if mem > 16 else "ramp.low"),
                                                         ("·" * (20 - int(mem / 80 * 20)), F)], (f"{cpu:4.0f}%", sp)])
    c.table(u, columns=[Col("USER", 8), Col("MEM", 6, "r"), Col("0 ─── 16G ────── 80G", 20), Col("CPU", 5, "r")],
            rows=rows, selected=1, cursor="❯")
    k = c.panel(kl, "Kernels (4)", role="list")
    c.table(k, columns=[Col("ID", 11), Col("ENV", 10), Col("RSS", 6, "r"), Col("AGE", 4, "r")], inactive=True,
            rows=[[("alice-4711", F), ("sci", F), ("12.4G", F), ("3h", F)], [("alice-4802", F), ("sci-legacy", F), ("28.1G", F), ("1h", F)],
                  [("bob-5120", F), ("sci", F), ("18.2G", F), ("40m", F)], [("carol-6001", F), ("sci", F), ("2.0G", F), ("5m", F)]])
    c.message(msg.y, "warn", "alice above guarantee: 41.2G (reclaim starts at 80G)", "now")
    c.keybar(keys.y, [("tab", "pane"), ("s", "stop kernel"), (":", "commands"), ("u", "users"), ("p", "policy")],
             region="keybar")
    note_invented(c)
    c.save(str(OUT / "dashboard--normal--120x30.mock"))


# ---------------------------------------------------------------- concept C
def concept_c():
    """Budget map: the policy itself is the screen; one memory bar split by user."""
    c = Canvas(120, 30, title="cm-top — budget map", state="normal", theme=THEME, caps=CAPS)
    hdr, tabs, bud, low, msg, keys = c.rect.split_v([1, 1, 9, "*", 1, 1])
    head(c, hdr.y, 120)
    c.tabs(1, tabs.y, ["Budget", "Users", "Kernels", "Hardware", "WSL", "Audit"], active=0)
    b = c.panel(bud, "Memory budget · user.slice max 104G of 110G WSL", role="chart", focus=True)
    w = b.w - 2
    segs = [("demo", 0.0, "ramp.low"), ("alice", 41.2, "accent.primary"), ("bob", 18.5, "accent.secondary"), ("carol", 2.1, "status.info")]
    x = b.x + 1
    for name, mem, tok in segs:
        n = max(1 if mem > 0.5 else 0, round(mem / 104 * w))
        if n:
            c.text(x, b.y + 1, "█" * n, tok)
            x += n
    c.text(x, b.y + 1, "░" * (b.x + 1 + w - x), F)
    g = round(16 * 4 / 104 * w)
    c.text(b.x + 1, b.y + 2, "└" + "─" * (g - 2) + "┘", "fg.muted")
    c.text(b.x + 1, b.y + 3, "guaranteed 4 × 16G = 64G", "fg.muted")
    c.text(b.x + 1, b.y + 5, "■ demo 0.0G   ", "ramp.low")
    c.runs(b.x + 15, b.y + 5, [("■ alice 41.2G   ", F), ("■ bob 18.5G   ", F), ("■ carol 2.1G   ", F), ("free 42.2G", "fg.muted")])
    c.runs(b.x + 1, b.y + 6, [("CPU  ", "fg.muted"), ("equal shares, weight 100 · idle cores are lent out", "fg.default")])
    pl, al = low.split_h([44, "*"], gap=1)
    p = c.panel(pl, "Policy", role="form")
    c.kv(p.x + 1, p.y, [("Guarantee", "16G per user"), ("Soft cap", "80G per user"), ("Total max", "104G"),
                        ("CPU weight", "100 (equal)"), ("GPU", "unlimited, shared"), ("oomd", [("✓ kill at 60% pressure", "status.success")])],
         key_w=11)
    a = c.panel(al, "Needs attention", role="list")
    c.table(a, columns=[Col("", 1), Col("WHAT", 44), Col("WHEN", 6, "r")], inactive=True,
            rows=[[c.status("warn"), ("alice above guarantee (41.2G)", F), ("now", F)],
                  [c.status("info"), ("bob kernel idle 40m, 18.2G held", F), ("40m", F)],
                  [c.status("ok"), ("policy applied to 4 users", "fg.default"), ("2m", "fg.muted")]])
    c.message(msg.y, None, "e edit policy · values apply live, no reboot", None)
    c.keybar(keys.y, [("e", "edit policy"), ("enter", "drill in"), ("1-6", "tab")], region="keybar")
    note_invented(c)
    c.save(str(OUT / "budget--normal--120x30.mock"))


if __name__ == "__main__":
    OUT.mkdir(exist_ok=True)
    concept_a(); concept_b(); concept_c()
