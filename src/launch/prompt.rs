use crate::role::Backend;

pub const PREAMBLE_CORE_ROLE: &str = include_str!("../../assets/core/preamble-core-role.md");
pub const PREAMBLE_ROLE: &str = include_str!("../../assets/core/preamble-role.md");
pub const OAT_META_BASELINE: &str = include_str!("../../assets/core/oat-meta-baseline.md");
pub const ROLE_PROTOCOL: &str = include_str!("../../assets/core/role-protocol.md");
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

/// Rewrites a `$skill-name` reference for the backend that spells it differently
/// (ADR-0002). No core skill in this ticket uses one, so this is a trivial identity today,
/// kept as the one hook `assemble` calls so a later ticket does not have to find a new seam.
pub fn rewrite_skill_references(text: &str, _backend: Backend) -> String {
    text.to_string()
}

/// preamble -> core baseline (core role) or role protocol (plugin role) -> the catalog's
/// instructions, joined in order -> the task (ADR-0004's fixed order).
pub fn assemble(parts: PromptParts<'_>, backend: Backend) -> String {
    let mut sections = vec![parts.preamble];
    sections.push(rewrite_skill_references(parts.baseline, backend));
    for instruction in parts.instructions {
        sections.push(rewrite_skill_references(instruction, backend));
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
pub fn render_preamble(template: &str, fields: PreambleFields<'_>) -> String {
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
    rewrite_skill_references(&rendered, fields.backend)
}
