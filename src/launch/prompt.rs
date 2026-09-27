// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

use crate::role::Backend;
use std::collections::BTreeSet;

pub const PREAMBLE_CORE_ROLE: &str = include_str!("../../assets/core/preamble-core-role.md");
pub const PREAMBLE_ROLE: &str = include_str!("../../assets/core/preamble-role.md");
pub const PREAMBLE_CONSOLE: &str = include_str!("../../assets/core/preamble-console.md");
pub const OAT_META_BASELINE: &str = include_str!("../../assets/core/oat-meta-baseline.md");
pub const OAT_CONSOLE_BASELINE: &str = include_str!("../../assets/core/oat-console-baseline.md");
pub const ROLE_PROTOCOL: &str = include_str!("../../assets/core/role-protocol.md");
pub const OAT_SYSTEM_VIEW_SKILL: &str =
    include_str!("../../assets/core/skills/oat-system-view/SKILL.md");
pub const LAUNCH_PROTOCOL_CLAUDE: &str =
    include_str!("../../assets/core/launch-protocol-claude.md");
pub const LAUNCH_PROTOCOL_CODEX: &str = include_str!("../../assets/core/launch-protocol-codex.md");

pub fn launch_protocol_for(backend: Backend) -> &'static str {
    match backend {
        Backend::Claude => LAUNCH_PROTOCOL_CLAUDE,
        Backend::Codex => LAUNCH_PROTOCOL_CODEX,
    }
}

/// The one input to prompt assembly (ticket §6.1): every launch path builds one of these and
/// hands it to `assemble`, which is the only place the four parts are concatenated, so the
/// order in the ticket's Contracts cannot drift between `meta fire` and `role fire`.
pub struct PromptParts<'a> {
    pub preamble: String,
    pub baseline: &'a str,
    pub instructions: &'a [String],
    pub task: &'a str,
}

/// Rewrites a `$skill-name` reference for the backend that spells it differently (ADR-0002):
/// Claude Code reads a plain `skill-name`, so the leading `$` is dropped; Codex reads the
/// reference as written, so the text passes through unchanged. Used both for instruction and
/// baseline text here, and for a skill's own `SKILL.md` at materialization time
/// (`launch::skills::write_skill_files`) — never for a skill's other files, which are not
/// prose and carry no such reference.
///
/// Only a `$name` where `name` is one of `known_skills` is rewritten — a bare regex over any
/// identifier-shaped run of characters would also corrupt a literal shell variable reference
/// (`$HOME`, `$PATH`, `$1`) that a role's instructions or a skill's own `SKILL.md` happen to
/// show in an example command.
pub fn rewrite_skill_references(text: &str, backend: Backend, known_skills: &BTreeSet<String>) -> String {
    if backend != Backend::Claude {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' && chars.peek().is_some_and(|n| is_skill_name_char(*n)) {
            let mut name = String::new();
            while let Some(&next) = chars.peek() {
                if is_skill_name_char(next) {
                    name.push(next);
                    chars.next();
                } else {
                    break;
                }
            }
            if known_skills.contains(&name) {
                out.push_str(&name);
            } else {
                out.push('$');
                out.push_str(&name);
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn is_skill_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// preamble -> core baseline (core role) or role protocol (plugin role) -> the catalog's
/// instructions, joined in order -> the task (ADR-0004's fixed order).
pub fn assemble(parts: PromptParts<'_>, backend: Backend, known_skills: &BTreeSet<String>) -> String {
    let mut sections = vec![parts.preamble];
    sections.push(rewrite_skill_references(parts.baseline, backend, known_skills));
    for instruction in parts.instructions {
        sections.push(rewrite_skill_references(instruction, backend, known_skills));
    }
    sections.push(parts.task.to_string());
    sections
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
}

/// Every named placeholder the preamble templates use, grouped so `render_preamble` takes one
/// argument instead of a long positional list.
pub struct PreambleFields<'a> {
    pub role_name: &'a str,
    pub run_id: &'a str,
    pub dispatch_id: &'a str,
    pub backend: Backend,
    pub worktree: &'a str,
    pub role_names: &'a [String],
    pub settle_command: &'a str,
}

/// Fills the preamble template's named placeholders. Kept separate from `assemble` so the
/// preamble text itself stays plain Markdown with no templating logic embedded in it.
pub fn render_preamble(template: &str, fields: PreambleFields<'_>, known_skills: &BTreeSet<String>) -> String {
    let mut role_list = fields.role_names.to_vec();
    role_list.sort();
    let rendered = template
        .replace("{{ROLE_NAME}}", fields.role_name)
        .replace("{{RUN_ID}}", fields.run_id)
        .replace("{{DISPATCH_ID}}", fields.dispatch_id)
        .replace("{{BACKEND_LABEL}}", fields.backend.label())
        .replace("{{WORKTREE}}", fields.worktree)
        .replace("{{ROLE_NAMES}}", &role_list.join(", "))
        .replace("{{SETTLE_COMMAND}}", fields.settle_command)
        .replace("{{LAUNCH_PROTOCOL}}", launch_protocol_for(fields.backend));
    rewrite_skill_references(&rendered, fields.backend, known_skills)
}
