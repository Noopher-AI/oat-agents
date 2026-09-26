use serde::Serialize;
use std::fmt;

/// A stable, machine-readable failure. Every failing command returns one of these
/// instead of an opaque `anyhow::Error`, so a caller can act on `code` without parsing text.
#[derive(Debug, Clone, Serialize)]
pub struct CliFailure {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl CliFailure {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({"code": self.code}))
    }
}

impl fmt::Display for CliFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CliFailure {}

/// Every error code this crate returns. Later tickets and role instructions name these by
/// value, so a rename here is a breaking change.
pub mod codes {
    pub const INVALID_CLI_ARGUMENTS: &str = "invalid_cli_arguments";
    pub const CLAP_DISPLAY: &str = "clap_display";
    pub const INVALID_INPUT: &str = "invalid_input";
    pub const RUN_NOT_BOUND: &str = "run_not_bound";
    pub const DISPATCH_NOT_BOUND: &str = "dispatch_not_bound";
    pub const ALREADY_SETTLED: &str = "already_settled";
    pub const RUN_STILL_OPEN: &str = "run_still_open";
    pub const INVALID_WORKTREE_ID: &str = "invalid_worktree_id";
    pub const INVALID_ROLE_OPTION: &str = "invalid_role_option";
    pub const ROLE_SOURCE_REQUIRED: &str = "role_source_required";
    pub const UNKNOWN_ROLE: &str = "unknown_role";
    pub const CORE_ROLE_INSTRUCTIONS_MISSING: &str = "core_role_instructions_missing";
    pub const SKILL_CONFLICT: &str = "skill_conflict";
    pub const SKILL_WRITE_FAILED: &str = "skill_write_failed";
    pub const TMUX_MISSING: &str = "tmux_missing";
    pub const INVALID_LOCAL_SETTINGS: &str = "invalid_local_settings";
    pub const TRANSCRIPT_NOT_FOUND: &str = "transcript_not_found";
    pub const META_NOT_FOUND: &str = "meta_not_found";
    pub const INTERNAL_ERROR: &str = "internal_error";
}

pub fn err(code: &str, message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(CliFailure::new(code, message))
}

/// Renders any error as the stable JSON failure shape on stderr. An error that is not a
/// `CliFailure` (a bug, an I/O surprise) becomes `internal_error` rather than leaking a
/// Rust `Debug` string as the contract.
pub fn to_failure(error: &anyhow::Error) -> CliFailure {
    if let Some(failure) = error.downcast_ref::<CliFailure>() {
        failure.clone()
    } else {
        CliFailure::new(codes::INTERNAL_ERROR, error.to_string())
    }
}
