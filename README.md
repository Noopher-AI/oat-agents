# oat-agents — Open Agent Team

`oat-agents` is a command-line tool that runs a supervised team of coding agents on one
machine. A coordinator agent takes one Big Plan, delegates bounded pieces of it to other
agents — each in its own git worktree and its own tmux session — and settles the result
through a file-based inbox.

The core ships two roles of its own: `oat-meta`, the coordinator, and `oat-console`, the
operator's system console. Every other role, and the skills they use, comes from a
**plugin**. A minimal example plugin provides `planner`, `worker` and `reviewer`.

## Status

The core lifecycle is in place: Runs, Dispatches, the Run inbox, the workflow log, git
worktrees and tmux sessions, plugins pinned per repository, the live view and `oat-console`, and
execution environments (pods) for build and test commands.

Build and install:

```sh
cargo build --release --locked
./install.sh                       # installs oat-agents and its human-facing skill
```

Declare a repository's plugins, then start a Run:

```sh
oat-agents init --repo . --git <plugin-url> <commit> <name>
oat-agents meta fire --prompt "…" --repo . [--agent claude|codex] [--exec-profile <name>]
oat-agents tui                     # watch it
```

## Pods

Roles that declare `exec_environment = true` run their build and test commands in a pod built
from the repository's `.devcontainer/` — but only when the Run has an execution profile. Set one
up once per machine and check it before the first Run:

```sh
oat-agents init --repo . --exec-profile local --exec-context <kube-context> --exec-namespace <namespace>
$EDITOR ~/.oat/exec.toml           # finish the [image] table; every field is there, commented
oat-agents env doctor --profile local
oat-agents env image  --profile local
```

Where commands run is never hidden: `meta fire` and `role fire` report it and warn when a role
that wanted a pod will run on the host, the workflow log records it, and the live view shows
`pod:…` or `HOST (no pod)` beside every Run and agent. See `docs/exec-environments.md`.

## Reading further

- `AGENTS.md` — how to work in this repository, including the verification commands
- `.dev_docs/CONTEXT.md` — the vocabulary, including the words not to use
- `.dev_docs/adr/` — architecture decisions
- `docs/plugins.md` — writing a plugin: the on-disk format and a worked example
- `docs/exec-environments.md` — pods: setting up a profile, choosing it for a Run, seeing it

## License

MIT.
