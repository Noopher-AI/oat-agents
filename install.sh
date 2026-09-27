#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 oat-agents contributors
#
# Builds oat-agents and installs it, plus the human-facing oat-agents-cli skill, into
# overridable directories. See --help.
#
# Run it from a checkout, or straight from the web without one:
#   curl -fsSL https://raw.githubusercontent.com/Noopher-AI/oat-agents/main/install.sh | sh
set -eu

BIN_DIR="${HOME}/.local/bin"
SKILL_ROOT="${HOME}/.agents/skills"
SKIP_SKILL=0
REPO_URL="${OAT_AGENTS_REPO:-https://github.com/Noopher-AI/oat-agents.git}"
REF="${OAT_AGENTS_REF:-main}"

usage() {
    cat <<'USAGE'
Usage: install.sh [OPTIONS]
       curl -fsSL https://raw.githubusercontent.com/Noopher-AI/oat-agents/main/install.sh | sh -s -- [OPTIONS]

Builds the oat-agents binary and installs it, plus the oat-agents-cli skill. Run from outside
a checkout, it fetches the source into a temporary directory first and removes it afterwards.

Options:
  --bin-dir <path>     Where to install the oat-agents binary (default: ~/.local/bin)
  --skill-root <path>  Where to install the oat-agents-cli skill (default: ~/.agents/skills)
  --skip-skill         Install only the binary, not the skill
  --ref <ref>          Branch, tag or commit to build when fetching the source (default: main)
  --help               Print this message

Environment:
  OAT_AGENTS_REPO      Where to fetch the source from (default: the GitHub repository)
USAGE
}

while [ $# -gt 0 ]; do
    case "$1" in
        --bin-dir)
            BIN_DIR="$2"
            shift 2
            ;;
        --skill-root)
            SKILL_ROOT="$2"
            shift 2
            ;;
        --skip-skill)
            SKIP_SKILL=1
            shift
            ;;
        --ref)
            REF="$2"
            shift 2
            ;;
        --help)
            usage
            exit 0
            ;;
        *)
            echo "install.sh: unknown option '$1'" >&2
            usage >&2
            exit 1
            ;;
    esac
done

need() {
    command -v "$1" >/dev/null 2>&1 || {
        echo "install.sh: '$1' is required but was not found. $2" >&2
        exit 1
    }
}
need cargo "Install Rust from https://rustup.rs and try again."

# A checkout is recognised by its own files, not by where the script happens to be: piped
# into sh, $0 is the shell and the working directory may be any project.
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
if ! grep -qs '^name = "oat-agents"' "$SCRIPT_DIR/Cargo.toml" \
    || [ ! -f "$SCRIPT_DIR/assets/core/skills/oat-agents-cli/SKILL.md" ]; then
    need git "It is needed to fetch the source, and oat-agents uses it at run time too."
    FETCHED="$(mktemp -d)"
    trap 'rm -rf "$FETCHED"' EXIT
    echo "Fetching oat-agents ($REF) from $REPO_URL..."
    git -C "$FETCHED" init -q
    git -C "$FETCHED" fetch -q --depth 1 "$REPO_URL" "$REF"
    git -C "$FETCHED" checkout -q FETCH_HEAD
    SCRIPT_DIR="$FETCHED"
fi

echo "Building oat-agents (cargo build --release --locked)..."
(cd "$SCRIPT_DIR" && cargo build --release --locked)

mkdir -p "$BIN_DIR"
# cargo resolves a relative CARGO_TARGET_DIR against the directory it was run in.
case "${CARGO_TARGET_DIR:-}" in
    "") TARGET_DIR="$SCRIPT_DIR/target" ;;
    /*) TARGET_DIR="$CARGO_TARGET_DIR" ;;
    *) TARGET_DIR="$SCRIPT_DIR/$CARGO_TARGET_DIR" ;;
esac
install -m 0755 "$TARGET_DIR/release/oat-agents" "$BIN_DIR/oat-agents"
echo "Installed oat-agents to $BIN_DIR/oat-agents"

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) echo "Note: $BIN_DIR is not on your PATH." >&2 ;;
esac

if [ "$SKIP_SKILL" -eq 0 ]; then
    SKILL_DEST="$SKILL_ROOT/oat-agents-cli"
    mkdir -p "$SKILL_DEST"
    cp "$SCRIPT_DIR/assets/core/skills/oat-agents-cli/SKILL.md" "$SKILL_DEST/SKILL.md"
    echo "Installed the oat-agents-cli skill to $SKILL_DEST/SKILL.md"
fi

for tool in git tmux; do
    command -v "$tool" >/dev/null 2>&1 || echo "Note: oat-agents needs $tool at run time; it was not found." >&2
done
if ! command -v claude >/dev/null 2>&1 && ! command -v codex >/dev/null 2>&1; then
    echo "Note: no agent CLI found; install Claude Code (claude) or Codex (codex) before a Run." >&2
fi
