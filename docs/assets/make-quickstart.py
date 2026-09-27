#!/usr/bin/env python3
"""Draws docs/assets/quickstart.svg: the getting-started commands typed out, one caption per step.

    python3 docs/assets/make-quickstart.py
"""
from html import escape
CW,LH,FS=8.4,20,14
PX,TOP=22,52
STEPS=[
 ("curl -fsSL https://raw.githubusercontent.com/Noopher-AI/oat-agents/main/install.sh | sh",
  [("dim","Fetching oat-agents (main) from https://github.com/Noopher-AI/oat-agents.git..."),
   ("dim","Building oat-agents (cargo build --release --locked)..."),
   ("green","Installed oat-agents to ~/.local/bin/oat-agents"),
   ("green","Installed the oat-agents-cli skill to ~/.agents/skills/oat-agents-cli/SKILL.md")],
  "① One command, no clone: the binary plus a skill that teaches your agent to drive it"),
 ("cd ~/src/webapp && oat-agents init --local", [("fg",'{ "plugins": [ { "name": "example", "source": "embedded" } ], "local": true, … }')],
  "② Pin the repository's plugins; with no flags, the built-in example team"),
 ("oat-agents plugin trust embedded --name example", [("fg",'{ "source": "embedded", "trusted": true, "version": "0.1.0" }')],
  "③ Trust each plugin once per machine: its instructions steer your agents"),
 ('oat-agents meta fire --name rate-limit-login --prompt "Rate-limit the login endpoint"', [("fg",'{ "run_id": "rate-limit-login", "branch": "oat/rate-limit-login/meta", "backend": "claude", … }')],
  "④ Fire a Run: oat-meta starts in its own worktree and tmux session"),
 ("oat-agents tui", [("cyan","▸ opening the live view…")],
  "⑤ Watch every agent, answer oat-meta's questions, type into any session"),
]
C={"bg":"#0d1117","fg":"#c9d1d9","dim":"#6e7681","green":"#3fb950","cyan":"#39c5cf","magenta":"#d2a8ff","white":"#e6edf3"}
COLS=100
rows=sum(1+len(o)+1 for _,o,_ in STEPS)
W=round(PX*2+COLS*CW); TERM_H=round(TOP+rows*LH); CAP=56; H=TERM_H+CAP
DUR=[4.0,2.8,2.8,3.4,3.6]; T=sum(DUR)+1.4
css=["text{font:%dpx ui-monospace,SFMono-Regular,'JetBrains Mono',Menlo,Consolas,'DejaVu Sans Mono',monospace;white-space:pre}"%FS,
     ".cap{font:20px -apple-system,BlinkMacSystemFont,'Segoe UI',Helvetica,Arial,sans-serif}",
     ".cur{animation:blink 1s steps(1) infinite}@keyframes blink{50%{opacity:0}}"]
body=[]; row=0; t=0
def pct(x): return x/T*100
for i,(cmd,out,cap) in enumerate(STEPS):
    d=DUR[i]; type_t=min(1.4,0.04*len(cmd)+.3)
    a=pct(t); te=pct(t+type_t); e=pct(t+d)
    # appear (stays until loop end)
    css.append(f"@keyframes s{i}{{0%,{max(a-.01,0):.2f}%{{opacity:0}}{a:.2f}%,99%{{opacity:1}}100%{{opacity:0}}}}.s{i}{{animation:s{i} {T}s infinite}}")
    css.append(f"@keyframes o{i}{{0%,{te:.2f}%{{opacity:0}}{te+.01:.2f}%,99%{{opacity:1}}100%{{opacity:0}}}}.o{i}{{animation:o{i} {T}s infinite}}")
    n=len(cmd)
    css.append(f"@keyframes k{i}{{0%,{a:.2f}%{{transform:translateX(0)}}{te:.2f}%{{transform:translateX({n*CW:.1f}px);opacity:1}}{te+.01:.2f}%,100%{{transform:translateX({n*CW:.1f}px);opacity:0}}}}.k{i}{{animation:k{i} {T}s infinite}}")
    last = i==len(STEPS)-1
    capend = 99 if last else e
    css.append(f"@keyframes c{i}{{0%,{max(a-.01,0):.2f}%{{opacity:0}}{a:.2f}%,{capend-.01:.2f}%{{opacity:1}}{capend:.2f}%,100%{{opacity:0}}}}.c{i}{{animation:c{i} {T}s infinite;opacity:0}}")
    y=TOP+row*LH
    g=[f'<text x="{PX}" y="{y}" fill="{C["magenta"]}">$</text>',
       f'<text x="{PX+2*CW:.1f}" y="{y}" fill="{C["white"]}">{escape(cmd)}</text>',
       f'<rect class="k{i}" x="{PX+2*CW:.1f}" y="{y-LH+5}" width="{n*CW+12:.1f}" height="{LH}" fill="{C["bg"]}"/>']
    o=[]
    for j,(col,line) in enumerate(out):
        o.append(f'<text x="{PX}" y="{y+(j+1)*LH}" fill="{C[col]}">{escape(line)}</text>')
    body.append(f'<g class="s{i}" opacity="0">{"".join(g)}<g class="o{i}">{"".join(o)}</g></g>')
    body.append(f'<text class="cap c{i}" x="{W//2}" y="{TERM_H+36}" fill="{C["fg"]}" text-anchor="middle">{escape(cap)}</text>')
    row+=1+len(out)+1; t+=d
svg=(f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" role="img" aria-label="Getting started: install.sh, oat-agents init, plugin trust, meta fire, then the live view.">'
 f'<style>{"".join(css)}</style>'
 f'<rect width="{W}" height="{H}" rx="12" fill="{C["bg"]}"/>'
 f'<path d="M0 {TERM_H}H{W}V{H-12}Q{W} {H} {W-12} {H}H12Q0 {H} 0 {H-12}Z" fill="#161b22"/><rect y="{TERM_H}" width="{W}" height="1" fill="#30363d"/>'
 f'<rect x=".5" y=".5" width="{W-1}" height="{H-1}" rx="12" fill="none" stroke="#30363d"/>'
 f'<circle cx="22" cy="18" r="6" fill="#ff5f57"/><circle cx="42" cy="18" r="6" fill="#febc2e"/><circle cx="62" cy="18" r="6" fill="#28c840"/>'
 f'<text x="{W/2:.0f}" y="23" fill="{C["dim"]}" text-anchor="middle" style="font-size:12px">getting started</text>'
 f'{"".join(body)}</svg>\n')
open(__import__('pathlib').Path(__file__).with_name('quickstart.svg'),'w',encoding='utf-8').write(svg)
print(W,H,T)
