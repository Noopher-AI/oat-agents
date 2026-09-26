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
worktrees and tmux sessions, and generic role launches through a `RoleCatalog` seam. The core
ships only `oat-meta`'s launch in this ticket; reading plugins from disk, the live view and
`oat-console`, and execution environments are later tickets (see `.dev_docs/adr/`).

Build and install:

```sh
cargo build --release --locked
./install.sh                       # installs oat-agents and its human-facing skill
```

Start a Run (once a plugin source exists to supply roles — F4/F5):

```sh
oat-agents meta fire --prompt "…" --repo . --agent claude
```

- `AGENTS.md` — how to work in this repository, including the verification commands
- `.dev_docs/CONTEXT.md` — the vocabulary, including the words not to use
- `.dev_docs/adr/` — architecture decisions
- `docs/plugins.md` — writing a plugin: the on-disk format and a worked example

## License

MIT.
