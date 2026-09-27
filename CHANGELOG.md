# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Contributor documentation for the public release: `CONTRIBUTING.md`, `SECURITY.md`,
  `CODE_OF_CONDUCT.md`, issue and pull request templates, `CODEOWNERS`, and Dependabot
  configuration. CI now also runs on pull requests.
- An SPDX license header naming the oat-agents contributors on every source file, checked in CI.
- The example plugin's team works a change through to the coordinator's branch: `oat-meta` skips
  the planner when the Big Plan is already concrete, fires independent pieces together, reviews
  from the worker's worktree, merges a passed branch into its own worktree, hands a correction
  the failed branch and the reviewer's findings, and raises `needs-human` after two failed
  corrections. The planner marks each step's check and dependencies, the reviewer leaves the
  worker's worktree untouched and lists each problem with its file and line, and the console
  names why a stuck Run is stuck.

### Changed

- The example plugin's `branch-naming` skill is replaced by `committing`. Its rule had a worker
  switch to a `<role>/<slug>` branch, leaving the Dispatch's own branch — the one the
  coordinator merges and cleanup checks — without the work.

### Fixed

- `cargo clippy` on the current stable toolchain passes again, so CI on `main` is green.

## [0.1.0]

### Added

- Initial public version: `oat-agents` launches a Big Plan with `oat-meta`, which delegates
  Dispatches of plugin-defined roles, each in its own git worktree and tmux session, and reads
  their results from the Run inbox. Includes the live view (`oat-agents tui`), the `oat-console`
  core role, the embedded `example` plugin, git-sourced plugins with per-machine trust, Claude
  Code and Codex backends, optional Kubernetes execution environments, and `install.sh`.
