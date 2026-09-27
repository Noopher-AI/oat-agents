#!/usr/bin/env python3
"""Draws docs/assets/tui-demo.svg: an animated replay of `oat-agents tui` for the README.

The layout, glyphs and colours follow src/tui (the header, the two-line roster, the tabs,
the timeline and the hint rows). The Run it shows is illustrative. Run it from anywhere:

    python3 docs/assets/make-tui-demo.py
"""

from html import escape
from pathlib import Path

COLS, ROWS = 104, 33
CW, LH, FS = 8.4, 19.0, 14
PAD_X, TOP = 18, 44
W = round(PAD_X * 2 + COLS * CW)
TERM_H = round(TOP + ROWS * LH + 16)
CAPTION_H = 56
H = TERM_H + CAPTION_H

C = {
    "bg": "#0d1117", "fg": "#c9d1d9", "dim": "#6e7681", "gray": "#8b949e",
    "white": "#e6edf3", "cyan": "#39c5cf", "magenta": "#d2a8ff", "green": "#3fb950",
    "yellow": "#e3b341", "blue": "#58a6ff", "red": "#f85149", "black": "#0d1117",
}
ROLE = {"oat-meta": "magenta", "planner": "cyan", "worker": "green", "reviewer": "yellow"}

RUN = "rate-limit-login"
META = "oat-meta-3c1e-rate-limit-login"
PLAN = "planner-8a02-plan-rate-limit"
WK_A = "worker-51d9-token-bucket"
WK_B = "worker-c7e4-login-429"
REV = "reviewer-2b6f-token-bucket"
WK_A2 = "worker-e03a-token-bucket-fix"
REV2 = "reviewer-9d41-token-bucket-fix"
HASHES = [META, PLAN, WK_A, WK_B, REV, WK_A2, REV2]
COLUMN = max(map(len, HASHES)) + 2

AGENTS = {
    META: ("oat-meta", "opus-5-5 high", ""),
    PLAN: ("planner", "sonnet-5", ""),
    WK_A: ("worker", "opus-5-5", "pod:local"),
    WK_B: ("worker", "gpt-5.5-codex", "pod:local"),
    REV: ("reviewer", "opus-5-5 high", "pod:local"),
    WK_A2: ("worker", "opus-5-5", "pod:local"),
    REV2: ("reviewer", "opus-5-5 high", "pod:local"),
}

# (clock, hash, glyph, glyph colour, event, detail)
EVENTS = [
    ("14:02:11", "·", "◆", "white", "run_created", f"{RUN}"),
    ("14:02:11", "·", "Σ", "cyan", "exec_profile_selected", "pod:local"),
    ("14:02:13", META, "▸", "green", "agent_enter", "oat-meta (claude)"),
    ("14:02:40", META, "◆", "magenta", "delegation", "planner: turn the Big Plan into steps"),
    ("14:02:41", PLAN, "▸", "green", "agent_enter", "planner (claude)"),
    ("14:06:02", PLAN, "✉", "cyan", "inbox_message", "completion"),
    ("14:06:03", PLAN, "✓", "blue", "agent_exit", "succeeded"),
    ("14:06:30", WK_A, "▸", "green", "agent_enter", "worker (claude)"),
    ("14:06:31", WK_B, "▸", "green", "agent_enter", "worker (codex)"),
    ("14:06:44", WK_A, "▣", "blue", "env_ready", "pod oat-51d9 ready"),
    ("14:19:52", WK_A, "✉", "cyan", "inbox_message", "completion"),
    ("14:19:58", REV, "▸", "green", "agent_enter", "reviewer (claude) in worker-51d9's worktree"),
    ("14:27:15", REV, "★", "yellow", "review-result", "FAIL: burst of 20 not refused under load"),
    ("14:27:20", META, "◆", "magenta", "retry", "worker: fix exactly what the review found"),
    ("14:27:21", WK_A2, "▸", "green", "agent_enter", "worker (claude)"),
    ("14:31:08", META, "🙋", "yellow", "needs_human", "429 or 503 for a locked account?"),
    ("14:32:40", META, "✅", "green", "human_answer", "429, with Retry-After"),
    ("14:36:11", REV2, "▸", "green", "agent_enter", "reviewer (claude) in worker-e03a's worktree"),
    ("14:38:02", REV2, "★", "yellow", "review-result", "PASS: re-ran tests in pod, 41 passed"),
]


class Frame:
    """One screen of the replay, drawn cell by cell as positioned spans."""

    def __init__(self):
        self.parts = []

    def text(self, col, row, s, color="fg", bold=False, bg=None):
        x = PAD_X + col * CW
        y = TOP + row * LH
        if bg:
            self.parts.append(
                f'<rect x="{x:.1f}" y="{y - LH + 5:.1f}" width="{len(s) * CW:.1f}" '
                f'height="{LH:.1f}" fill="{C[bg]}"/>'
            )
        weight = ' font-weight="700"' if bold else ""
        self.parts.append(
            f'<text x="{x:.1f}" y="{y:.1f}" fill="{C[color]}"{weight}>{escape(s)}</text>'
        )

    def spinner(self, col, row):
        x, y = PAD_X + col * CW, TOP + row * LH
        self.parts.append(f'<use href="#spin" x="{x:.1f}" y="{y:.1f}"/>')

    def box(self, top, bottom, title, lit):
        color = "cyan" if lit else "dim"
        self.text(0, top, "┌", color)
        self.text(1 + len(title), top, "─" * (COLS - 2 - len(title)) + "┐", color)
        for row in range(top + 1, bottom):
            self.text(0, row, "│", color)
            self.text(COLS - 1, row, "│", color)
        self.text(0, bottom, "└" + "─" * (COLS - 2) + "┘", color)
        self.text(1, top, title, "white" if lit else "gray", bold=lit)

    def svg(self):
        return "".join(self.parts)


def human_span(seconds):
    m, s = divmod(seconds, 60)
    h, m = divmod(m, 60)
    return f"{h}h{m:02d}m" if h else f"{m}m{s:02d}s" if m else f"{s}s"


def draw(state):
    """state: dict with keys agents (list of (hash, status, seconds, tokens, cost, outcome)),
    selected (index into rows, 0 = all agents), tab, tabs, events (count), page (optional
    list of (color, text) lines for the live tab), asking, notice, totals."""
    f = Frame()
    agents = state["agents"]
    active = sum(1 for a in agents if a[1] in ("run", "idle", "ask"))
    f.text(0, 0, f" {RUN} ", "black", bg="cyan")
    head = f"  {len(agents)} agents ({active} active)  {state['events']} events"
    f.text(len(RUN) + 2, 0, head, "fg")
    f.text(len(RUN) + 2 + len(head), 0, f"  {state['tokens']} tokens  {state['cost']}", "yellow")
    f.text(len(RUN) + 2 + len(head) + len(state["tokens"]) + len(state["cost"]) + 11, 0,
           "  times +08:00", "fg")

    live = [a for a in agents if a[1] in ("run", "idle", "ask")]
    done = [a for a in agents if a[1] not in ("run", "idle", "ask")]
    rule = bool(live and done)
    inner = min(len(agents) * 2 + 1 + rule, 13)
    top, bottom = 1, 1 + inner + 1
    f.box(top, bottom, " agents ", True)
    rows = [("all", None)] + [("agent", a) for a in live] + ([("rule", None)] if rule else []) + [
        ("agent", a) for a in done]
    row = top + 1
    selectable = 0
    for kind, agent in rows:
        if row >= bottom or (kind == "agent" and row + 1 >= bottom):
            break
        if kind == "rule":
            f.text(1, row, "─" * 6 + " finished " + "─" * (COLS - 18), "dim")
            row += 1
            continue
        selected = selectable == state["selected"]
        selectable += 1
        if selected:
            f.parts.append(
                f'<rect x="{PAD_X + CW:.1f}" y="{TOP + row * LH - LH + 5:.1f}" '
                f'width="{(COLS - 2) * CW:.1f}" height="{LH * (1 if kind == "all" else 2):.1f}" '
                f'fill="#1f2a37"/>'
            )
        if kind == "all":
            f.text(1, row, "all agents", "white" if selected else "gray")
            row += 1
            continue
        hash_id, status, seconds, tokens, cost, outcome = agent
        role, model, exec_ = AGENTS[hash_id]
        color = ROLE[role]
        if status == "ask":
            f.text(1, row, "🙋", "yellow")
        elif status == "run":
            f.spinner(1, row)
        elif status == "idle":
            f.text(1, row, "●", "green")
        elif status == "fail":
            f.text(1, row, "✗", "red")
        else:
            f.text(1, row, "✓", "dim")
        f.text(3, row, hash_id, color, bold=True)
        running = status in ("run", "idle", "ask")
        f.text(3 + COLUMN, row, f"{human_span(seconds):>7}", "white" if running else "dim")
        f.text(3 + COLUMN + 9, row, role, color)
        f.text(3 + COLUMN + 19, row, model, "white")
        second = f"{tokens:>6} {cost:>8}  "
        f.text(3, row + 1, second, "white")
        verdict = "running" if running else f"{outcome}"
        f.text(3 + len(second), row + 1, f"{verdict:<20}",
               "white" if running else ("red" if status == "fail" else "dim"))
        if exec_:
            f.text(3 + len(second) + 22, row + 1, exec_, "blue")
        row += 2

    tab_row = bottom + 1
    col = 0
    for i, name in enumerate(state["tabs"]):
        label = f" {name} "
        chosen = name == state["tab"]
        f.text(col, tab_row, label, "cyan" if chosen else "dim", bold=chosen)
        col += len(label)
        if i < len(state["tabs"]) - 1:
            f.text(col, tab_row, "│", "dim")
            col += 1

    page_top, page_bottom = tab_row + 1, ROWS - 4
    sel_hash = None if state["selected"] == 0 else ([a[0] for a in live] + [a[0] for a in done])[
        state["selected"] - 1]
    title = f" {sel_hash} · {state['tab']} " if sel_hash else f" {state['tab']} "
    f.box(page_top, page_bottom, title, state.get("typing", False))
    body = page_bottom - page_top - 1
    if state["tab"] == "timeline":
        shown = EVENTS[: state["events_shown"]][-body:]
        for i, (clock, h, glyph, gcolor, event, detail) in enumerate(shown):
            r = page_top + 1 + i
            f.text(1, r, clock, "dim")
            hcolor = ROLE.get(AGENTS[h][0], "gray") if h in AGENTS else "gray"
            f.text(10, r, h, hcolor)
            f.text(10 + COLUMN, r, glyph, gcolor)
            f.text(12 + COLUMN, r, event, gcolor)
            f.text(12 + COLUMN + max(14, len(event) + 1), r, detail, "fg")
    else:
        for i, (color, line) in enumerate(state["page"][:body]):
            f.text(1, page_top + 1 + i, line, color)

    hints = state["hints"]
    for i, line in enumerate(hints):
        col = 1
        for key, desc, sep in line:
            if sep:
                f.text(col, ROWS - 3 + i, sep, "dim")
                col += len(sep)
            f.text(col, ROWS - 3 + i, key, "cyan", bold=True)
            col += len(key) + 1
            f.text(col, ROWS - 3 + i, desc, "gray")
            col += len(desc)
    if state.get("notice"):
        f.text(1, ROWS - 1, state["notice"], "yellow")
    return f.svg()


TIMELINE_HINTS = [
    [("↑↓", "agent", ""), ("PgUp/PgDn", "page", " • "), ("g/G", "top/end", " • "),
     ("e", "events:all", " │ "), ("f", "follow:on", " • "), ("x", "close run", " • ")],
    [("ctrl+\\", "console", ""), ("ctrl+l", "checklist", " • "), ("esc", "runs", " • "),
     ("q", "quit", " • ")],
]
LIVE_HINTS = [
    [("↑↓", "agent", ""), ("PgUp/wheel", "scroll back", " • "), ("enter", "type", " │ "),
     ("v/drag", "select", " • "), ("←→", "tab:live", " • "), ("x", "close run", " • ")],
    [("ctrl+\\", "console", ""), ("ctrl+l", "checklist", " • "), ("esc", "runs", " • "),
     ("q", "quit", " • ")],
]
TYPING_HINTS = [
    [("keys", "go to the agent", ""), ("wheel/shift+PgUp", "scroll back", " • "),
     ("drag", "select & copy", " • "), ("ctrl+]", "stop typing", " │ ")],
    [("ctrl+\\", "console", "")],
]

ALL_TABS = ["live", "diff", "timeline", "log"]

WORKER_LIVE = [
    ("white", "● Update(src/auth/limiter.rs)"),
    ("dim", "  ⎿  Updated src/auth/limiter.rs with 38 additions"),
    ("green", "      + pub struct TokenBucket { capacity: u32, refill_per_sec: f64, … }"),
    ("green", "      + impl TokenBucket { pub fn try_take(&mut self, now: Instant) -> bool"),
    ("white", "● Bash(oat-agents env exec -- cargo test limiter)"),
    ("dim", "  ⎿  running in pod oat-e03a"),
    ("dim", "     test limiter::refuses_burst_over_capacity ... ok"),
    ("dim", "     test limiter::refills_after_one_second ... ok"),
    ("dim", "     test result: ok. 41 passed; 0 failed"),
    ("white", "● Bash(oat-agents dispatch done --report-file report.md)"),
    ("cyan", "✽ Settling… (esc to interrupt)"),
]

META_LIVE = [
    ("white", "● The correction round depends on one product decision I can't"),
    ("white", "  make from the Big Plan: what a locked account should get back."),
    ("fg", ""),
    ("white", "● Bash(oat-agents log record --event needs-human --message \"429 or 503 …?\")"),
    ("yellow", "  ⎿  🙋 waiting for the operator"),
    ("fg", ""),
    ("dim", "────────────────────────────────────────────────────────────────────────"),
    ("white", "> 429, with Retry-After. Same as the per-IP limit."),
    ("dim", "────────────────────────────────────────────────────────────────────────"),
]


def roster(stage):
    """The agents as each stage of the replay has them."""
    rows = {
        0: [(META, "run", 42, "38k", "$0.412", "")],
        1: [(META, "idle", 250, "96k", "$1.02", ""), (PLAN, "run", 201, "122k", "$0.318", "")],
        2: [(META, "idle", 1050, "180k", "$1.94", ""), (WK_A, "run", 820, "1.1M", "$3.87", ""),
            (WK_B, "run", 818, "640k", "$1.21", ""),
            (PLAN, "done", 202, "131k", "$0.341", "succeeded at 14:06")],
        3: [(META, "idle", 1510, "240k", "$2.61", ""), (REV, "run", 440, "410k", "$1.66", ""),
            (WK_B, "run", 1270, "910k", "$1.73", ""),
            (WK_A, "done", 1168, "1.4M", "$4.52", "succeeded at 14:19"),
            (PLAN, "done", 202, "131k", "$0.341", "succeeded at 14:06")],
        4: [(META, "idle", 1510 + 80, "252k", "$2.74", ""), (WK_A2, "run", 25, "44k", "$0.188", ""),
            (WK_B, "run", 1350, "990k", "$1.88", ""),
            (REV, "fail", 437, "418k", "$1.70", "FAIL at 14:27"),
            (WK_A, "done", 1168, "1.4M", "$4.52", "succeeded at 14:19"),
            (PLAN, "done", 202, "131k", "$0.341", "succeeded at 14:06")],
        5: [(META, "ask", 1740, "281k", "$3.05", ""), (WK_A2, "run", 230, "380k", "$1.44", ""),
            (WK_B, "done", 1480, "1.0M", "$1.97", "succeeded at 14:31"),
            (REV, "fail", 437, "418k", "$1.70", "FAIL at 14:27"),
            (WK_A, "done", 1168, "1.4M", "$4.52", "succeeded at 14:19"),
            (PLAN, "done", 202, "131k", "$0.341", "succeeded at 14:06")],
    }
    return rows[stage]


FRAMES = [
    # (seconds on screen, state)
    (1.6, dict(agents=roster(0), events=3, events_shown=3, tokens="38k", cost="$0.412")),
    (2.2, dict(agents=roster(1), events=5, events_shown=5, tokens="218k", cost="$1.34")),
    (2.6, dict(agents=roster(2), events=10, events_shown=10, tokens="2.1M", cost="$7.36")),
    (2.6, dict(agents=roster(3), events=12, events_shown=12, tokens="3.1M", cost="$11.06")),
    (2.8, dict(agents=roster(4), events=15, events_shown=15, tokens="3.3M", cost="$11.39")),
    (3.2, dict(agents=roster(4), events=15, events_shown=15, tokens="3.3M", cost="$11.39",
               selected=2, tab="live", tabs=ALL_TABS, page=WORKER_LIVE, hints=LIVE_HINTS)),
    (2.2, dict(agents=roster(5), events=16, events_shown=16, tokens="4.2M", cost="$13.39",
               notice="oat-meta is asking: 429 or 503 for a locked account?")),
    (3.4, dict(agents=roster(5), events=16, events_shown=16, tokens="4.2M", cost="$13.39",
               selected=1, tab="live", tabs=ALL_TABS, page=META_LIVE, hints=TYPING_HINTS,
               typing=True)),
    (4.0, dict(agents=[(META, "idle", 2160, "310k", "$3.36", ""),
                       (REV, "done", 360, "402k", "$1.61", "PASS at 14:38")] + roster(5)[1:],
               events=19, events_shown=19, tokens="5.1M", cost="$16.02")),
]
# The last frame: the new worker finished, then a fresh reviewer passed it.
FRAMES[-1][1]["agents"] = [
    (META, "idle", 2160, "310k", "$3.36", ""),
    (REV2, "done", 111, "402k", "$1.61", "PASS at 14:38"),
    (WK_A2, "done", 590, "720k", "$2.51", "succeeded at 14:32"),
    (WK_B, "done", 1480, "1.0M", "$1.97", "succeeded at 14:31"),
    (REV, "fail", 437, "418k", "$1.70", "FAIL at 14:27"),
    (WK_A, "done", 1168, "1.4M", "$4.52", "succeeded at 14:19"),
]


# One line under the terminal for each frame, saying what just happened.
CAPTIONS = [
    "① meta fire starts a Run: oat-meta gets its own worktree and tmux session",
    "② oat-meta delegates the first step to the plugin's planner role",
    "③ The plan comes back via the Run inbox; two workers start, on Claude Code and Codex",
    "④ A worker settles; an independent reviewer starts inside its worktree",
    "⑤ The review fails; the fix goes to a new Dispatch, the failed one stays on record",
    "⑥ Any agent's live terminal is one keypress away; its row says where tests ran",
    "⑦ 🙋 oat-meta needs a decision only you can make, and the view flags it",
    "⑧ Press enter and type the answer straight into oat-meta's session",
    "⑨ A fresh reviewer re-runs the tests in a pod and passes the fix; you merge",
]


def caption(text):
    return (
        f'<text class="cap" x="{W / 2:.0f}" y="{TERM_H + 36}" fill="{C["fg"]}" '
        f'text-anchor="middle">{escape(text)}</text>'
    )


def main():
    total = sum(d for d, _ in FRAMES)
    css = [
        f"text{{font:{FS}px ui-monospace,SFMono-Regular,'JetBrains Mono',Menlo,Consolas,"
        "'DejaVu Sans Mono',monospace;white-space:pre}",
        ".f{opacity:0}",
        ".cap{font:20px -apple-system,BlinkMacSystemFont,'Segoe UI',Helvetica,Arial,sans-serif}",
        ".sp text{opacity:0;animation:sp 1.2s steps(1) infinite}",
        "@keyframes sp{0%{opacity:1}10%{opacity:0}100%{opacity:0}}",
    ]
    groups = []
    start = 0.0
    for i, (duration, state) in enumerate(FRAMES):
        state.setdefault("selected", 0)
        state.setdefault("tab", "timeline")
        state.setdefault("tabs", ["timeline"])
        state.setdefault("hints", TIMELINE_HINTS)
        a = start / total * 100
        b = (start + duration) / total * 100
        on = f"{a:.3f}%" if i else "0%"
        css.append(
            f"@keyframes f{i}{{0%{{opacity:{1 if i == 0 else 0}}}"
            + (f"{a - 0.001:.3f}%{{opacity:0}}{on}{{opacity:1}}" if i else "")
            + f"{b - 0.001:.3f}%{{opacity:1}}{b:.3f}%{{opacity:0}}100%{{opacity:0}}}}"
            f".f{i}{{animation:f{i} {total:.1f}s infinite}}"
        )
        groups.append(f'<g class="f f{i}">{draw(state)}{caption(CAPTIONS[i])}</g>')
        start += duration
    spinner = "".join(
        f'<text fill="{C["green"]}" style="animation-delay:{i * 0.12:.2f}s">{ch}</text>'
        for i, ch in enumerate("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
    )
    chrome = (
        f'<rect width="{W}" height="{H}" rx="12" fill="{C["bg"]}"/>'
        f'<path d="M0 {TERM_H}H{W}V{H - 12}Q{W} {H} {W - 12} {H}H12Q0 {H} 0 {H - 12}Z" fill="#161b22"/>'
        f'<rect x="0" y="{TERM_H}" width="{W}" height="1" fill="#30363d"/>'
        f'<rect x=".5" y=".5" width="{W - 1}" height="{H - 1}" rx="12" fill="none" stroke="#30363d"/>'
        f'<circle cx="22" cy="18" r="6" fill="#ff5f57"/><circle cx="42" cy="18" r="6" fill="#febc2e"/>'
        f'<circle cx="62" cy="18" r="6" fill="#28c840"/>'
        f'<text x="{W / 2:.0f}" y="23" fill="{C["dim"]}" text-anchor="middle" '
        f'style="font-size:12px">oat-agents tui</text>'
    )
    svg = (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" '
        f'viewBox="0 0 {W} {H}" role="img" aria-label="An animated replay of the oat-agents '
        f'live view: oat-meta delegates to a planner, two workers and a reviewer; the review '
        f'fails, a correction round is fired, oat-meta asks the operator one question, and the '
        f'second review passes.">'
        f"<style>{''.join(css)}</style>"
        f'<defs><g id="spin" class="sp">{spinner}</g></defs>'
        f"{chrome}{''.join(groups)}</svg>\n"
    )
    out = Path(__file__).with_name("tui-demo.svg")
    out.write_text(svg, encoding="utf-8")
    print(f"wrote {out} ({len(svg) // 1024} KiB, {total:.1f}s loop)")


if __name__ == "__main__":
    main()
