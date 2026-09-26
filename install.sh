#!/bin/sh
# Builds oat-agents and installs it, plus the human-facing oat-agents-cli skill, into
# overridable directories. See --help.
set -eu

BIN_DIR="${HOME}/.local/bin"
SKILL_ROOT="${HOME}/.agents/skills"
SKIP_SKILL=0

usage() {
    cat <<'USAGE'
Usage: install.sh [OPTIONS]

Builds the oat-agents binary and installs it, plus the oat-agents-cli skill.

Options:
  --bin-dir <path>     Where to install the oat-agents binary (default: ~/.local/bin)
  --skill-root <path>  Where to install the oat-agents-cli skill (default: ~/.agents/skills)
  --skip-skill         Install only the binary, not the skill
  --help               Print this message
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

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "Building oat-agents (cargo build --release --locked)..."
(cd "$SCRIPT_DIR" && cargo build --release --locked)

mkdir -p "$BIN_DIR"
install -m 0755 "$SCRIPT_DIR/target/release/oat-agents" "$BIN_DIR/oat-agents"
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
