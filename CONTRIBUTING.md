# Contributing to oat-agents

Thanks for your interest in contributing to oat-agents.

## Read first

- [`AGENTS.md`](AGENTS.md): the rules for working in this repository. They apply to people as
  much as to agents.
- [`.dev_docs/CONTEXT.md`](.dev_docs/CONTEXT.md): the vocabulary. Use its terms in code, tests,
  issues and pull requests, and mind its *Avoid* lines.
- [`.dev_docs/adr/`](.dev_docs/adr/): the decision records. If your change contradicts one,
  say so in the issue before writing code.

## Development environment

- **Rust** 1.85 or newer (edition 2024). A devcontainer is provided in `.devcontainer/`.
- **git** and **tmux**, which oat-agents drives at runtime.
- Optionally the `claude` or `codex` CLI, to launch real agents while trying a change by hand.

## Building and testing

Run these from the repository root; CI runs the same commands:

```sh
cargo build --locked --all-targets
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
.github/scripts/check-dev-docs.sh
```

## Core or plugin?

The core owns the mechanism; how a team works belongs in a plugin (ADR-0001). Before changing the
core, check whether the change would encode one team's practice. If it would, it belongs in a
plugin, and the core may need a hook for it instead. See [`docs/plugins.md`](docs/plugins.md).

## Branching and commits

- Branch off `main`.
- Follow [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`,
  `refactor:`, `chore:`, etc.
- Every commit must be signed off (see DCO below).
- Never commit credentials, machine names, cluster contexts or private repository paths.
  Everything pushed here is public and stays public.

## Developer Certificate of Origin (DCO)

oat-agents does not require a CLA. Instead, every commit must include a `Signed-off-by` line
certifying, under the [Developer Certificate of Origin](https://developercertificate.org/), that
you wrote it or otherwise have the right to submit it under the project's MIT license. Add it
automatically with:

```sh
git commit -s
```

## Pull request process

1. Open an issue to discuss the change before starting significant work.
2. Fork the repository and create a branch for your change.
3. Open a pull request against `main`.
4. Make sure CI is green.
5. Address review feedback.

Every pull request should also include:

- Tests for the behavior being added or fixed.
- A `CHANGELOG.md` entry under `[Unreleased]` for a user-visible change.
- A new or updated decision record in `.dev_docs/adr/` if the change makes or reverses an
  architectural decision (see [`.dev_docs/adr/README.md`](.dev_docs/adr/README.md)).
- A DCO sign-off on every commit.
