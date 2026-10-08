#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 oat-agents contributors

"""Draws the README's two still figures:

- docs/assets/one-session.svg: many agents inside one session, beside the same team on one
  screen in oat-agents.
- docs/assets/architecture.svg: a plugin is a directory, and the team it launches.

    python3 docs/assets/make-figures.py
"""

from html import escape
from pathlib import Path

C = {
    "bg": "#0d1117", "panel": "#161b22", "line": "#30363d", "fg": "#c9d1d9", "dim": "#6e7681",
    "gray": "#8b949e", "white": "#e6edf3", "cyan": "#39c5cf", "magenta": "#d2a8ff",
    "green": "#3fb950", "yellow": "#e3b341", "blue": "#58a6ff", "red": "#f85149",
}
ROLE = {"meta": "magenta", "planner": "cyan", "worker": "green", "reviewer": "yellow",
        "security-auditor": "blue"}
MONO = ("font-family:ui-monospace,SFMono-Regular,'JetBrains Mono',Menlo,Consolas,"
        "'DejaVu Sans Mono',monospace;white-space:pre")
SANS = "font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Helvetica,Arial,sans-serif"
STYLE = (f"<style>text{{{MONO};font-size:13px}}.s{{{SANS}}}.h{{{SANS};font-size:17px;"
         f"font-weight:600}}.n{{{SANS};font-size:15px}}</style>")


def text(x, y, s, color="fg", cls="", bold=False, anchor=None, size=None):
    attrs = f' class="{cls}"' if cls else ""
    attrs += ' font-weight="700"' if bold else ""
    attrs += f' text-anchor="{anchor}"' if anchor else ""
    attrs += f' style="font-size:{size}px"' if size else ""
    return f'<text x="{x:.1f}" y="{y:.1f}" fill="{C[color]}"{attrs}>{escape(s)}</text>'


def frame(w, h, label):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" '
            f'viewBox="0 0 {w} {h}" role="img" aria-label="{escape(label)}">{STYLE}'
            f'<rect width="{w}" height="{h}" rx="12" fill="{C["bg"]}"/>'
            f'<rect x=".5" y=".5" width="{w - 1}" height="{h - 1}" rx="12" fill="none" '
            f'stroke="{C["line"]}"/>')


def panel(x, y, w, h, stroke="line"):
    return (f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="8" fill="{C["panel"]}" '
            f'stroke="{C[stroke]}"/>')


def one_session():
    w, h = 960, 440
    out = [frame(w, h, "Left: many agents inside one session, where every subagent writes into "
                 "the same scrollback, nobody's state is visible and none can be stepped into. "
                 "Right: the same team in oat-agents, one row per agent with its state, each its "
                 "own session on its own branch, and enter takes over any of them.")]
    # Left: one session, everyone's output interleaved.
    out.append(text(36, 44, "Many agents inside one session", "white", "h"))
    out.append(panel(24, 60, 444, 260))
    flood = [
        ("planner", "reading src/auth/*.rs (14 files)"),
        ("worker", "edit limiter.rs  +38 −4"),
        ("planner", "step 3 of 4: 429 on the login route"),
        ("worker-2", "cargo test … running"),
        ("worker", "cargo build … 212 lines of output"),
        ("reviewer", "reading the diff (612 lines)"),
        ("worker-2", "3 failed, retrying"),
        ("planner", "done? waiting for worker"),
        ("worker", "… +214 lines"),
        ("reviewer", "which branch has the fix?"),
        ("worker-2", "cargo test … running"),
    ]
    for i, (who, what) in enumerate(flood):
        y = 86 + i * 20
        role = who.split("-")[0]
        out.append(text(40, y, "▸", "dim"))
        out.append(text(56, y, f"{who:<9}", ROLE.get(role, "gray")))
        out.append(text(56 + 10 * 7.8, y, what, "fg"))
    out.append('<defs><linearGradient id="fade" x1="0" y1="0" x2="0" y2="1">'
               f'<stop offset="0" stop-color="{C["panel"]}" stop-opacity="0"/>'
               f'<stop offset="1" stop-color="{C["panel"]}"/></linearGradient></defs>'
               '<rect x="25" y="250" width="442" height="69" rx="8" fill="url(#fade)"/>')
    for i, line in enumerate(["Everything lands in one scrollback",
                              "You can't tell who is stuck",
                              "You can't step into a subagent"]):
        y = 352 + i * 28
        out.append(text(36, y, "✗", "red", "n", bold=True))
        out.append(text(58, y, line, "fg", "n"))
    # Right: the same team on one screen.
    out.append(text(504, 44, "The same team in oat-agents", "white", "h"))
    out.append(panel(492, 60, 444, 260, "cyan"))
    team = [
        ("◆", "magenta", "meta-7a1c-rate-limit-login", "meta", "reading its mail"),
        ("✓", "dim", "planner-8a02-plan", "planner", "done · 4 steps"),
        ("●", "green", "worker-51d9-token-bucket", "worker", "running · 12m"),
        ("●", "green", "worker-c7e4-login-429", "worker", "running · 12m"),
        ("🙋", "yellow", "reviewer-2b6f-login-429", "reviewer", "asked a question"),
    ]
    for i, (glyph, gcolor, hash_id, role, state) in enumerate(team):
        y = 92 + i * 46
        out.append(text(508, y, glyph, gcolor))
        out.append(text(530, y, hash_id, ROLE[role], bold=True))
        out.append(text(530, y + 18, f"⎇ own branch  ▣ own session  · {state}", "dim"))
    for i, line in enumerate(["One row per agent, its state at a glance",
                              "Each agent is a real session of its own",
                              "enter takes over any of them; ctrl+] gives it back"]):
        y = 352 + i * 28
        out.append(text(504, y, "✓", "green", "n", bold=True))
        out.append(text(526, y, line, "fg", "n"))
    out.append("</svg>\n")
    return "".join(out)


def architecture():
    w, h = 960, 500
    out = [frame(w, h, "A plugin is a directory of Markdown and TOML: core/oat-meta-instruction.md "
                 "says how the meta-agent splits the work and when it asks you, and each "
                 "directory under roles/ is one kind of member agent. The core launches that team "
                 "the same way for every plugin: worktrees, tmux sessions, the Run inbox, the "
                 "workflow log, pods and the live view.")]
    out.append(text(36, 44, "Your plugin: a directory", "white", "h"))
    out.append(panel(24, 60, 456, 330))
    tree = [
        ("plugins/your-team/", "white", ""),
        ("├─ oat-plugin.toml", "fg", ""),
        ("├─ core/", "fg", ""),
        ("│  └─ oat-meta-instruction.md", "magenta", ""),
        ("│        how to split the work, when to ask you", "dim", ""),
        ("├─ roles/", "fg", ""),
        ("│  ├─ planner/", "cyan", "role.toml + instructions.md"),
        ("│  ├─ worker/", "green", "max_concurrent = 4"),
        ("│  ├─ reviewer/", "yellow", "exec_environment = true"),
        ("│  └─ security-auditor/", "blue", "a new role: a new directory"),
        ("└─ skills/", "fg", ""),
        ("   └─ committing/SKILL.md", "fg", "know-how a role may load"),
    ]
    for i, (line, color, note) in enumerate(tree):
        y = 88 + i * 25
        out.append(text(44, y, line, color))
        if note:
            out.append(text(464, y, note, "dim", anchor="end"))
    # The arrow from the files to the team.
    out.append(f'<path d="M488 225H516" stroke="{C["gray"]}" stroke-width="2" fill="none"/>'
               f'<path d="M516 219L526 225L516 231Z" fill="{C["gray"]}"/>')
    out.append(text(548, 44, "The team it launches", "white", "h"))
    out.append(panel(536, 60, 400, 330))
    cx = 736
    out.append(f'<rect x="{cx - 92}" y="80" width="184" height="44" rx="8" fill="{C["bg"]}" '
               f'stroke="{C["magenta"]}"/>')
    out.append(text(cx, 99, "meta-agent", "magenta", bold=True, anchor="middle"))
    out.append(text(cx, 116, "splits the goal, reads mail", "dim", anchor="middle", size=11))
    members = [("planner", "×1"), ("worker", "×4"), ("reviewer", "×1"), ("security-auditor", "×1")]
    out.append(f'<path d="M{cx} 124V{264 + 30}" stroke="{C["line"]}" stroke-width="1.5" fill="none"/>')
    for i, (role, count) in enumerate(members):
        x = 556 + (i % 2) * 190
        y = 172 + (i // 2) * 92
        edge = x + 170 if i % 2 == 0 else x
        out.append(f'<path d="M{cx} {y + 30}H{edge}" stroke="{C["line"]}" stroke-width="1.5" '
                   'fill="none"/>')
        out.append(f'<rect x="{x}" y="{y}" width="170" height="60" rx="8" fill="{C["bg"]}" '
                   f'stroke="{C[ROLE[role]]}"/>')
        out.append(text(x + 85, y + 22, f"{role} {count}", ROLE[role], bold=True, anchor="middle"))
        out.append(text(x + 85, y + 42, "⎇ worktree · ▣ session", "dim", anchor="middle", size=11))
    out.append(text(cx, 376, "member agents, each on the backend its role names", "dim",
                    anchor="middle", size=11))
    # The core, the same underneath every team.
    out.append(panel(24, 408, 912, 72))
    out.append(text(44, 436, "The core, the same for every team", "white", "s", bold=True,
                    size=14))
    out.append(text(44, 462, "▸ launch  ⎇ worktrees  ▣ tmux sessions  ✉ Run inbox  "
                    "≡ workflow log  ⬢ pods, if a role needs one  ◉ live view", "gray"))
    out.append("</svg>\n")
    return "".join(out)


def main():
    here = Path(__file__).parent
    for name, svg in (("one-session.svg", one_session()), ("architecture.svg", architecture())):
        (here / name).write_text(svg, encoding="utf-8")
        print(f"wrote {here / name}")


if __name__ == "__main__":
    main()
