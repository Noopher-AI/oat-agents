#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 oat-agents contributors

"""Draws docs/assets/hero.svg: the whole team on one screen, one key into an agent's own
session, one key back. The Run it shows is illustrative; the glyphs, colours and keys follow
src/tui. Run it from anywhere:

    python3 docs/assets/make-hero.py
"""

from html import escape
from pathlib import Path

W, H = 960, 540
TERM_H = 484
CW, FS = 8.4, 14
T = 13.0  # seconds per loop

C = {
    "bg": "#0d1117", "panel": "#161b22", "line": "#30363d", "fg": "#c9d1d9", "dim": "#6e7681",
    "gray": "#8b949e", "white": "#e6edf3", "cyan": "#39c5cf", "magenta": "#d2a8ff",
    "green": "#3fb950", "yellow": "#e3b341", "blue": "#58a6ff", "sel": "#1f2a37",
}

css = [
    f"text{{font:{FS}px ui-monospace,SFMono-Regular,'JetBrains Mono',Menlo,Consolas,"
    "'DejaVu Sans Mono',monospace;white-space:pre}",
    ".cap{font:20px -apple-system,BlinkMacSystemFont,'Segoe UI',Helvetica,Arial,sans-serif}",
    ".key{font:13px -apple-system,BlinkMacSystemFont,'Segoe UI',Helvetica,Arial,sans-serif}",
    ".sp text{opacity:0;animation:sp 1.2s steps(1) infinite}",
    "@keyframes sp{0%{opacity:1}10%{opacity:0}100%{opacity:0}}",
    ".cur{animation:blink 1s steps(1) infinite}@keyframes blink{50%{opacity:0}}",
]
_n = 0


def pct(seconds):
    return seconds / T * 100


def window(a, b):
    """A class visible from second a to second b of every loop."""
    global _n
    _n += 1
    name = f"w{_n}"
    if a <= 0:
        frames = f"0%,{pct(b) - .01:.2f}%{{opacity:1}}{pct(b):.2f}%,100%{{opacity:0}}"
    elif b >= T:
        frames = f"0%,{pct(a) - .01:.2f}%{{opacity:0}}{pct(a):.2f}%,100%{{opacity:1}}"
    else:
        frames = (f"0%,{pct(a) - .01:.2f}%{{opacity:0}}{pct(a):.2f}%,{pct(b) - .01:.2f}%"
                  f"{{opacity:1}}{pct(b):.2f}%,100%{{opacity:0}}")
    css.append(f"@keyframes {name}{{{frames}}}.{name}{{animation:{name} {T}s infinite}}")
    return name


def text(x, y, s, color="fg", bold=False, cls="", anchor=None):
    weight = ' font-weight="700"' if bold else ""
    klass = f' class="{cls}"' if cls else ""
    anchor = f' text-anchor="{anchor}"' if anchor else ""
    return f'<text x="{x:.1f}" y="{y:.1f}" fill="{C[color]}"{weight}{klass}{anchor}>{escape(s)}</text>'


def box(x, y, w, h, title, color="line", title_color="gray"):
    return (f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="6" fill="none" '
            f'stroke="{C[color]}"/>'
            f'<rect x="{x + 12}" y="{y - 9}" width="{len(title) * CW + 4:.1f}" height="18" '
            f'fill="{C["bg"]}"/>' + text(x + 14, y + 5, title, title_color, bold=True))


def keycap(label, a, b):
    w = len(label) * 8 + 28
    x, y = W - 40 - w, 424
    cls = window(a, b)
    return (f'<g class="{cls}"><rect x="{x}" y="{y}" width="{w}" height="30" rx="6" '
            f'fill="{C["panel"]}" stroke="{C["cyan"]}" stroke-width="1.5"/>'
            f'<text class="key" x="{x + w / 2}" y="{y + 20}" fill="{C["white"]}" '
            f'text-anchor="middle" font-weight="700">{escape(label)}</text></g>')


# --- The team: one row per agent, each with its own branch and session. ---------------
ROSTER_X, ROW_Y, ROW_STEP = 24, 122, 54
HASH_X, ROLE_X, STATE_X = 64, 316, 412
ROWS = [
    # glyph, glyph colour, hash_id, role, colour, state, branch
    ("◆", "magenta", "meta-7a1c-rate-limit-login", "oat-meta", "magenta", "reading its mail", "meta"),
    ("✓", "dim", "planner-8a02-plan", "planner", "cyan", "done · 4 steps", "plan"),
    (None, "green", "worker-51d9-token-bucket", "worker", "green", "running · 12m", "token-bucket"),
    (None, "green", "worker-c7e4-login-429", "worker", "green", "running · 12m", "login-429"),
    (None, "green", "reviewer-2b6f-login-429", "reviewer", "yellow", "running · 3m", "login-429"),
]
TAKEN = 2  # the row the operator takes over
BACK = 9.8  # when the view is back on the whole team


def roster():
    parts = [box(ROSTER_X, 92, 636, 300, " agents ", "cyan", "white")]
    sel_meta, sel_worker = window(0, 3.3), window(3.3, T)
    for cls, row in ((sel_meta, 0), (sel_worker, TAKEN)):
        y = ROW_Y + row * ROW_STEP - 17
        parts.append(f'<rect class="{cls}" x="{ROSTER_X + 8}" y="{y}" width="620" height="44" '
                     f'rx="4" fill="{C["sel"]}"/>')
    for i, (glyph, gcolor, hash_id, role, color, state, branch) in enumerate(ROWS):
        y = ROW_Y + i * ROW_STEP
        if glyph:
            parts.append(text(42, y, glyph, gcolor))
        elif i == TAKEN:
            parts.append(f'<g class="{window(0, BACK)}"><use href="#spin" x="42" y="{y}"/></g>')
            parts.append(f'<g class="{window(BACK, T)}">{text(42, y, "✓", "dim")}</g>')
        else:
            parts.append(f'<use href="#spin" x="42" y="{y}"/>')
        parts.append(text(HASH_X, y, hash_id, color, bold=True))
        parts.append(text(ROLE_X, y, role, color))
        if i == TAKEN:
            parts.append(f'<g class="{window(0, BACK)}">{text(STATE_X, y, state, "white")}</g>')
            parts.append(f'<g class="{window(BACK, T)}">{text(STATE_X, y, "done · 41 tests pass", "dim")}</g>')
        else:
            parts.append(text(STATE_X, y, state, "dim" if glyph == "✓" else "white"))
        where = f"⎇ oat/rate-limit-login/{branch}  ▣ own worktree, own tmux session"
        parts.append(text(HASH_X, y + 20, where, "dim"))
    return "".join(parts)


# --- The Run inbox: member agents write to the meta-agent, which replies. ----------------
INBOX_X = 676


def inbox():
    parts = [box(INBOX_X, 92, 260, 300, " Run inbox ", "line", "gray")]
    mail = [
        (0.0, "✉ planner", "member_done", "plan in 4 steps", "cyan"),
        (1.2, "✉ worker-c7e4", "question", "429 or 503 if locked?", "cyan"),
        (2.4, "↩ meta-agent", "reply", "429, with Retry-After", "magenta"),
        (BACK + 0.6, "✉ worker-51d9", "member_done", "41 tests pass", "cyan"),
    ]
    for i, (at, who, kind, gist, color) in enumerate(mail):
        y = 126 + i * 62
        body = (text(INBOX_X + 16, y, who, color, bold=True)
                + text(INBOX_X + 16 + (len(who) + 1) * CW, y, kind, "white")
                + text(INBOX_X + 34, y + 20, gist, "dim"))
        parts.append(f'<g class="{window(at, T)}">{body}</g>' if at else body)
    parts.append(text(INBOX_X + 16, 376, "read by the meta-agent only", "dim"))
    return "".join(parts)


def header():
    chip = " rate-limit-login "
    return (f'<rect x="24" y="52" width="{len(chip) * CW:.1f}" height="20" rx="3" fill="{C["cyan"]}"/>'
            + text(24, 67, chip, "bg", bold=True)
            + text(24 + len(chip) * CW + 12, 67, "1 meta-agent · 4 member agents", "fg")
            + text(W - 24, 67, "goal: Rate-limit the login endpoint", "gray", anchor="end"))


def hints():
    keys = [("↑↓", "agent"), ("enter", "take over"), ("ctrl+]", "back"), ("ctrl+\\", "console")]
    x, parts = 24, []
    for key, desc in keys:
        parts.append(text(x, 434, key, "cyan", bold=True))
        x += (len(key) + 1) * CW
        parts.append(text(x, 434, desc, "gray"))
        x += (len(desc) + 3) * CW
    return "".join(parts)


# --- One agent's own session, full screen, as you type into it. ---------------------------
ZOOM_IN, LIVE_END = 4.4, 9.3


def live():
    x, y, w, h = 24, 48, 912, 422
    origin = f"{HASH_X + 120}px {ROW_Y + TAKEN * ROW_STEP}px"
    a, b = pct(ZOOM_IN), pct(ZOOM_IN + 0.45)
    c, d = pct(LIVE_END), pct(LIVE_END + 0.45)
    css.append(
        "@keyframes zoom{"
        f"0%,{a:.2f}%{{transform:scale(.04);opacity:0}}"
        f"{a + .01:.2f}%{{transform:scale(.04);opacity:1}}"
        f"{b:.2f}%,{c:.2f}%{{transform:scale(1);opacity:1}}"
        f"{d - .01:.2f}%{{transform:scale(.04);opacity:1}}{d:.2f}%,100%{{transform:scale(.04);opacity:0}}}}"
        f".zoom{{transform-origin:{origin};animation:zoom {T}s infinite}}"
    )
    lines = [
        ("white", "● Read(src/auth/limiter.rs)"),
        ("white", "● Update(src/auth/limiter.rs)"),
        ("dim", "  ⎿  +38 −4"),
        ("green", "      + pub struct TokenBucket { capacity: u32, refill_per_sec: f64 }"),
        ("white", "● Bash(oat-agents env exec -- cargo test limiter)"),
        ("dim", "  ⎿  running in pod oat-51d9"),
        ("dim", "     test result: ok. 41 passed; 0 failed"),
    ]
    title = " worker-51d9-token-bucket · live · typing "
    parts = [f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="6" fill="{C["bg"]}" '
             f'stroke="{C["cyan"]}" stroke-width="1.5"/>',
             f'<rect x="{x + 12}" y="{y - 9}" width="{len(title) * CW + 4:.1f}" height="18" '
             f'fill="{C["bg"]}"/>',
             text(x + 14, y + 5, title, "white", bold=True)]
    for i, (color, line) in enumerate(lines):
        parts.append(text(x + 24, y + 44 + i * 24, line, color))
    rule = "─" * 104
    prompt_y = y + 44 + 8.5 * 24
    parts.append(text(x + 24, prompt_y - 24, rule, "dim"))
    parts.append(text(x + 24, prompt_y + 24, rule, "dim"))
    typed = "also clamp the refill to capacity, then run the tests again"
    start, end = ZOOM_IN + 1.0, ZOOM_IN + 3.0
    n = len(typed)
    css.append(
        f"@keyframes type{{0%,{pct(start):.2f}%{{transform:translateX(0)}}"
        f"{pct(end):.2f}%,100%{{transform:translateX({n * CW:.1f}px)}}}}"
        f".type{{animation:type {T}s infinite}}")
    parts.append(f'<clipPath id="pane"><rect x="{x}" y="{y}" width="{w - 2}" height="{h}"/></clipPath>')
    parts.append(f'<g class="{window(0, end + 0.6)}" clip-path="url(#pane)">'
                 + text(x + 24, prompt_y, "> " + typed, "white")
                 + f'<rect class="type" x="{x + 24 + 2 * CW:.1f}" y="{prompt_y - 16}" '
                 f'width="{n * CW + 10:.1f}" height="22" fill="{C["bg"]}"/></g>')
    parts.append(f'<g class="{window(end + 0.6, T)}">' + text(x + 24, prompt_y, "> ", "white")
                 + f'<rect class="cur" x="{x + 24 + 2 * CW:.1f}" y="{prompt_y - 13}" width="8" '
                 f'height="16" fill="{C["white"]}"/>'
                 + text(x + 24, prompt_y + 60, "> " + typed, "gray")
                 + text(x + 24, prompt_y + 88, "● Clamping refill() at capacity, then re-running the tests.", "white")
                 + "</g>")
    parts.append(text(x + 24, y + h - 18, "keys go to the agent", "gray"))
    parts.append(text(x + 24 + 21 * CW, y + h - 18, "•", "dim"))
    parts.append(text(x + 24 + 23 * CW, y + h - 18, "ctrl+]", "cyan", bold=True))
    parts.append(text(x + 24 + 30 * CW, y + h - 18, "stop typing", "gray"))
    return f'<g class="zoom">{"".join(parts)}</g>'


CAPTIONS = [
    (0, ZOOM_IN, "① The whole team on one screen: a meta-agent and the members it sent out"),
    (ZOOM_IN, LIVE_END, "② One key, and you are typing into that agent's own native session"),
    (LIVE_END, T, "③ One key back. Nobody else stopped, and the work came in by mail"),
]


def main():
    spinner = "".join(
        f'<text fill="{C["green"]}" style="animation-delay:{i * 0.12:.2f}s">{ch}</text>'
        for i, ch in enumerate("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"))
    overview = header() + roster() + inbox() + hints()
    session = live()
    keys = keycap("enter", 3.4, ZOOM_IN + 0.3) + keycap("ctrl+]", LIVE_END - 0.7, LIVE_END + 0.5)
    caps = "".join(
        f'<g class="{window(a, b)}"><text class="cap" x="{W / 2:.0f}" y="{TERM_H + 36}" '
        f'fill="{C["fg"]}" text-anchor="middle">{escape(s)}</text></g>' for a, b, s in CAPTIONS)
    chrome = (
        f'<rect width="{W}" height="{H}" rx="12" fill="{C["bg"]}"/>'
        f'<path d="M0 {TERM_H}H{W}V{H - 12}Q{W} {H} {W - 12} {H}H12Q0 {H} 0 {H - 12}Z" fill="{C["panel"]}"/>'
        f'<rect y="{TERM_H}" width="{W}" height="1" fill="{C["line"]}"/>'
        f'<rect x=".5" y=".5" width="{W - 1}" height="{H - 1}" rx="12" fill="none" stroke="{C["line"]}"/>'
        f'<circle cx="22" cy="18" r="6" fill="#ff5f57"/><circle cx="42" cy="18" r="6" fill="#febc2e"/>'
        f'<circle cx="62" cy="18" r="6" fill="#28c840"/>'
        f'<text x="{W / 2:.0f}" y="23" fill="{C["dim"]}" text-anchor="middle" style="font-size:12px">'
        f"oat-agents tui</text>")
    svg = (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" '
        'role="img" aria-label="oat-agents: the whole team on one screen, a meta-agent and the '
        "member agents it sent out, each on its own branch, worktree and tmux session, writing "
        "to the meta-agent through the Run inbox. One key takes you into one agent's own native "
        'session to type into it; one key brings you back to the team.">'
        f"<style>{''.join(css)}</style>"
        f'<defs><g id="spin" class="sp">{spinner}</g></defs>'
        f"{chrome}{overview}{session}{keys}{caps}</svg>\n")
    out = Path(__file__).with_name("hero.svg")
    out.write_text(svg, encoding="utf-8")
    print(f"wrote {out} ({len(svg) // 1024} KiB, {T:.1f}s loop)")


if __name__ == "__main__":
    main()
