<p align="center">
  <b>English</b> · <a href="README.zh-TW.md">繁體中文</a>
</p>

<p align="center">
  <img src="docs/assets/hero.svg" alt="The oat-agents live view: the whole team on one screen, a meta-agent and the member agents it sent out, each on its own branch, worktree and tmux session, writing to the meta-agent through the Run inbox. One key takes you into one agent's own session to type into it; one key brings you back to the team." width="100%">
</p>

<h1 align="center">oat-agents</h1>

<p align="center">
  <b>One screen for the whole team. One key to take over any agent.</b><br>
  A meta-agent splits your goal across a team of coding agents. Each one is a real session of
  its own agent CLI, on its own branch. You watch them all from one terminal and step into any
  of them whenever you want.
</p>

<p align="center">
  <a href="#why">Why</a> ·
  <a href="#how-a-run-goes">How a Run goes</a> ·
  <a href="#your-team-your-rules">Your team, your rules</a> ·
  <a href="#what-runs-where">What runs where</a> ·
  <a href="#get-started-in-five-commands">Get started</a>
</p>

---

## Why

Coding agents can already spawn subagents. That works for one or two. With five on a real
change, it gets hard to follow.

<p align="center">
  <img src="docs/assets/one-session.svg" alt="Left: many agents inside one session, where every subagent writes into the same scrollback, nobody's state is visible and none can be stepped into. Right: the same team in oat-agents, one row per agent with its state, each its own session on its own branch, and enter takes over any of them." width="100%">
</p>

Inside one session, every subagent's output lands in the same scrollback. You can't see which
one is stuck, and when one goes wrong you can't talk to it directly. All you can do is ask the
parent to pass a message along.

oat-agents takes the subagents out of the session. Each agent runs in its own tmux session,
with its own git worktree and branch, in the agent CLI's own interactive interface. A
**meta-agent** leads: it splits your goal into pieces and hands each one to a **member
agent**. The live view puts the whole team on one screen, one row per agent.

There is no new UI for the agents themselves. When you select one and press `enter`, you're
typing into its real session, the same as if you had opened it yourself. `ctrl+]` brings you
back to the team.

## How a Run goes

<p align="center">
  <img src="docs/assets/tui-demo.svg" alt="An animated replay of oat-agents tui: the meta-agent delegates to a planner, two workers on different backends and a reviewer; the review fails, a correction goes to a new member agent, the meta-agent asks the operator one question, and a fresh reviewer passes the fix." width="100%">
</p>

That is `oat-agents tui`. The Run is illustrative, but the layout, glyphs and keys are the
real ones.

1. **You give it a goal.** `oat-agents meta fire --prompt "…"` starts a Run. Describe the
   outcome you want, not the steps.
2. **The meta-agent splits the work.** It launches member agents with `role fire`: a planner,
   two workers, a reviewer, or whatever roles your plugin defines. It doesn't write the code
   itself.
3. **Members report by mail.** A member that finishes sends its report to the Run inbox
   (`dispatch done`). One that is blocked asks a question there (`dispatch ask`) and waits for
   the meta-agent's reply. Only the meta-agent reads the inbox.
4. **You're asked only when it matters.** When the meta-agent needs a decision it can't make
   from the goal, its row shows 🙋. Select it, press `enter`, type the answer.
5. **A failure stays on record.** A failed review sends the fix to a *new* member agent. The
   failed attempt keeps its row, its report and its place in the workflow log.
6. **You merge.** The meta-agent closes the Run with `meta finish` and prints a receipt. The
   work is on the Run's `oat/<run>/…` branches for you to review.

You choose how close to work. Stay at the top: read the roster, open the meta-agent's
checklist (`ctrl+l`), or ask the meta-agent where things stand. Or go down to any member, read
its diff, and work with it directly.

## Your team, your rules

<p align="center">
  <img src="docs/assets/architecture.svg" alt="A plugin is a directory of Markdown and TOML: core/oat-meta-instruction.md says how the meta-agent splits the work and when it asks you, and each directory under roles/ is one kind of member agent. The core launches that team the same way for every plugin: worktrees, tmux sessions, the Run inbox, the workflow log, pods and the live view." width="100%">
</p>

How your team works lives in a **plugin**: a directory of Markdown and TOML. It says which
roles exist, what each one is told, which model it runs on, and how the meta-agent should
split work and when it should ask you. The core supplies the mechanism and nothing else
(ADR-0001, ADR-0004).

The example plugin ships `planner`, `worker` and `reviewer` so oat-agents does something
useful on day one. **They are an example, not a fixed set.** A plugin can define a `tester`,
a `security-auditor`, a `migrator` that runs four at a time, or a team with no planner at all.

A new role is one directory with two files:

```text
team/
├── oat-plugin.toml
├── core/
│   └── oat-meta-instruction.md          # tell the meta-agent when to use your roles
└── roles/
    └── security-auditor/
        ├── role.toml
        └── instructions.md
```

```toml
# team/roles/security-auditor/role.toml
description = "Audits a change for security issues and reports findings with evidence."
start = "existing"          # run inside the worktree of the change under audit
exec_environment = false
max_concurrent = 1

[model.claude]
model = "opus"
```

```markdown
<!-- team/roles/security-auditor/instructions.md -->
# Security auditor

You start in the worktree of the change under audit. Look for injection, authorization gaps
and leaked secrets. Report each finding with the file, the line, and why it is exploitable.
```

Pin it next to the example team (or on its own), trust it once, and the next Run can use it:

```sh
oat-agents init --force --embedded --path team my-team
oat-agents plugin list --repo .          # prints the exact trust command for each plugin
```

| Want to change…                                       | Where                                                                 |
|-------------------------------------------------------|-----------------------------------------------------------------------|
| Which roles exist and what they do                    | `roles/<role>/role.toml` + `instructions.md` in your plugin           |
| How the meta-agent splits work, when it asks, when it gives up | `core/oat-meta-instruction.md` in your plugin                |
| Reusable know-how a role may load                     | `skills/<skill>/SKILL.md`, listed in the role's `skills = [...]`      |
| Model, reasoning effort, or backend, per role         | `[model.claude]` / `[model.codex]` in `role.toml`, or override per repository in `.oat/roles.toml` |
| How many of one role run at once                      | `max_concurrent`, overridable per repository                          |
| Share one team across repositories                    | Publish the plugin as a git repo and pin it with `init --git <url> <commit\|latest> <name>` |

Every TOML file rejects unknown fields, so a typo fails at load time instead of being silently
ignored. The full format is in [`docs/plugins.md`](docs/plugins.md).

## What runs where

- **One binary, no service.** `oat-agents` is a single Rust executable. A Run is branches, tmux
  sessions and plain files under `~/.local/state/oat-agents/`, including an append-only JSONL
  workflow log you can `tail -f`, `jq` or `grep`. There is no server, database or daemon.
- **Every agent in its own session.** Each member agent gets a worktree on its own branch
  (`oat/<run>/<name>`) and a tmux session on oat-agents' private tmux server. `meta fire` and
  `role fire` print the exact command to attach to it (`tmux -L oat attach -t oat_<…>`).
- **A pod only when a role needs one.** A role that sets `exec_environment = true` runs its
  build and test commands in a pod built from your `.devcontainer/` (Kubernetes today), when
  the Run has an execution profile. Everything else, the agent included, stays on your machine.
  The live view shows `pod:…` beside every agent, or `HOST (no pod)` in yellow when a role
  wanted a pod and didn't get one.
- **Honest verification.** The role protocol says a check the environment could not run is
  reported as *unverified*, never as a pass.
- **Agents run unattended.** Nobody is there to approve each step, so oat-agents starts your
  agent CLI with its permission prompts turned off. A worktree keeps each agent's work apart;
  it doesn't limit what the agent can reach. Run oat-agents on a machine, and with credentials,
  you'd trust an agent with.
- **Backends.** Claude Code and Codex today, chosen per role, so one team can mix them.

## Get started in five commands

<p align="center">
  <img src="docs/assets/quickstart.svg" alt="An animated terminal: install.sh, oat-agents init, plugin trust, meta fire, then the live view." width="100%">
</p>

**You need:** `git`, `tmux`, a Rust toolchain ([rustup.rs](https://rustup.rs), 1.85+, to
build), and at least one agent CLI installed and logged in: Claude Code (the default) or
Codex.

**1. Install with one command, no clone needed.** The script fetches the source into a
temporary directory, builds it, puts `oat-agents` in `~/.local/bin`, installs the
`oat-agents-cli` skill so your own coding agent can explain and drive oat-agents for you, and
then deletes the temporary directory.

```sh
curl -fsSL https://raw.githubusercontent.com/Noopher-AI/oat-agents/main/install.sh | sh
```

<details>
<summary>Other ways to install</summary>

```sh
# Pass options after `sh -s --`: another install directory, a tag or commit, binary only
curl -fsSL https://raw.githubusercontent.com/Noopher-AI/oat-agents/main/install.sh \
  | sh -s -- --bin-dir /usr/local/bin --ref <tag-or-commit> --skip-skill

# With cargo alone (binary only, into ~/.cargo/bin)
cargo install --locked --git https://github.com/Noopher-AI/oat-agents

# From a checkout
git clone https://github.com/Noopher-AI/oat-agents && cd oat-agents && ./install.sh
```

</details>

**2. Point it at a repository.** With no flags, `init` pins the built-in example team.
`--local` keeps the config under `.git/`, so your `git status` stays clean.

```sh
cd ~/src/your-project
oat-agents init --local
```

**3. Trust the plugin once on this machine.** Its instructions will steer your agents, so
oat-agents won't run a plugin you haven't approved.

```sh
oat-agents plugin trust embedded --name example
```

**4. Start a Run with a goal.** Describe the outcome, not the steps. Use `--input-file` for a
longer goal, or `--agent codex` to run the meta-agent on Codex.

```sh
oat-agents meta fire --name rate-limit-login \
  --prompt "Rate-limit the login endpoint: 5 attempts a minute per account, with tests."
```

**5. Watch the team.**

```sh
oat-agents tui
```

`↑↓` picks an agent and `←→` switches between its `live`, `diff`, `timeline` and `log` tabs.
`enter` types into the selected session, `ctrl+]` gives the keys back, `ctrl+l` opens the
checklist and `ctrl+\` opens the console.

> **Tip:** Ask your own coding agent "how do I use oat-agents?". The `oat-agents-cli` skill
> that `install.sh` installed walks it through starting, observing and taking over a Run.

### Optional: run checks in pods

Set up an execution profile once per machine, and roles that ask for one run their build and
test commands in a pod:

```sh
oat-agents init --repo . --exec-profile local --exec-context <kube-context> --exec-namespace <namespace>
$EDITOR ~/.oat/exec.toml           # finish the [image] table; every field is there, commented
oat-agents env doctor --profile local
oat-agents env image  --profile local
```

`meta fire` and `role fire` report where commands will run, and warn when a role that wanted a
pod will run on the host. See [`docs/exec-environments.md`](docs/exec-environments.md).

## Words you'll see

| Word | Meaning |
|---|---|
| **Goal** | What you want done, written for the meta-agent. It states the outcome, not the steps. |
| **Run** | One execution of one goal, from `meta fire` to `meta finish`. |
| **Meta-agent** | The agent that leads a Run: it splits the goal, launches member agents and reads their mail. It runs the core role `oat-meta`. |
| **Member agent** | An agent the meta-agent launched for one piece of work, as one of your plugin's roles. |
| **Dispatch** | One launch of one role: one session, one worktree, one settlement. A retry is a new Dispatch. |
| **Run inbox** | Mail from member agents to the meta-agent: reports and questions. Only the meta-agent reads it. |
| **Workflow log** | The append-only record of the Run. It's what you and the live view read. |

The full glossary, including the words we deliberately avoid, is in
[`.dev_docs/CONTEXT.md`](.dev_docs/CONTEXT.md).

## Reading further

- [`docs/plugins.md`](docs/plugins.md): writing a plugin, with the on-disk format and a worked example
- [`docs/exec-environments.md`](docs/exec-environments.md): pods, from setting up a profile to choosing one for a Run and seeing it
- [`.dev_docs/adr/`](.dev_docs/adr/): the architecture decisions
- [`AGENTS.md`](AGENTS.md): how to work in this repository, including the verification commands

## Contributing

Issues and pull requests are welcome. Start with [`CONTRIBUTING.md`](CONTRIBUTING.md); report
vulnerabilities privately as described in [`SECURITY.md`](SECURITY.md). Everyone taking part is
expected to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

[MIT](LICENSE).
