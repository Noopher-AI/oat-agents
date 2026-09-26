# Working in oat-agents

This file is the rules for every agent working in this repository, whichever backend runs it.

## Read before you change anything

1. `.dev_docs/CONTEXT.md` — the vocabulary. Use its terms in code, tests, issues and pull
   requests. The *Avoid* lines matter more than the definitions: each one names a word that
   has already caused a concrete mistake.
2. `.dev_docs/adr/` — every decision record touching the area you are about to change. If
   your change contradicts one, stop and say so; do not override it silently.

## This repository is public

Everything pushed here — every branch, commit, issue and pull request — is public and stays
public, even after it is deleted.

- Never commit credentials, machine names, cluster contexts or private repository paths.

## Core and plugins

- The core owns the mechanism: launching, worktrees, sessions, the Run inbox, the workflow
  log, execution environments, the live view, and the two core roles `oat-meta` and
  `oat-console` (ADR-0001).
- Everything that expresses *how a team works* — role instructions, working conventions,
  skills, model choices — belongs in a plugin. If a change to the core would encode one
  team's practice, it belongs in a plugin instead, and the core may need a hook for it.
- Names the core owns carry its prefix: `oat-agents` (command, crate, state directory),
  `oat-` (core roles, core skills, tmux, branches), `OAT_` (environment variables), `.oat/`
  (configuration). Plugin roles and plugin skills do **not** take the `oat-` prefix.

## Branches

- `main` is the base. Work for a spec lands on `epic/S<n>-<slug>` through one pull request per
  ticket; the epic reaches `main` in one pull request that a person merges.
- Decision records written during a spec's discussion live on its epic branch until it
  merges. Until then they are proposals and may be rewritten in place.

## Decision records

- Never delete or rewrite a record that has reached `main`; follow `.dev_docs/adr/README.md`.
- Cite a record by number (`ADR-0001`), never by filename. CI checks every citation resolves
  and every record is listed in the index.

## Verification

```sh
cargo build --locked --all-targets
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
.github/scripts/check-dev-docs.sh
```
