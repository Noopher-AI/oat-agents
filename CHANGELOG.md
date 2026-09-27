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

### Fixed

- `cargo clippy` on the current stable toolchain passes again, so CI on `main` is green.

## [0.1.0]

### Added

- Initial public version: `oat-agents` launches a Big Plan with `oat-meta`, which delegates
  Dispatches of plugin-defined roles, each in its own git worktree and tmux session, and reads
  their results from the Run inbox. Includes the live view (`oat-agents tui`), the `oat-console`
  core role, the embedded `example` plugin, git-sourced plugins with per-machine trust, Claude
  Code and Codex backends, optional Kubernetes execution environments, and `install.sh`.
