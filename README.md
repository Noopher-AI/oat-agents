# oat-agents — Open Agent Team

`oat-agents` is a command-line tool that runs a supervised team of coding agents on one
machine. A coordinator agent takes one Big Plan, delegates bounded pieces of it to other
agents — each in its own git worktree and its own tmux session — and settles the result
through a file-based inbox.

The core ships two roles of its own: `oat-meta`, the coordinator, and `oat-console`, the
operator's system console. Every other role, and the skills they use, comes from a
**plugin**. A minimal example plugin provides `planner`, `worker` and `reviewer`.

## Status

This repository is being bootstrapped. **There is no code yet, on purpose**: the first
feature ticket brings it in, so that ticket's review shows whether this skeleton was
followed. Until then the repository holds only its ground rules:

- `AGENTS.md` — how to work in this repository
- `.dev_docs/CONTEXT.md` — the vocabulary, including the words not to use
- `.dev_docs/adr/` — architecture decisions

## License

MIT.
