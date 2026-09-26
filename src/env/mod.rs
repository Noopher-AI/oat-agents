//! Execution environments: containers a Dispatch's build and test commands run in instead of
//! the host (`.dev_docs/CONTEXT.md`, *Execution environment*).
//!
//! The agent stays outside the environment and only its commands go in, so everything here is
//! about one question: when a role runs `cargo test`, which container did it actually run in,
//! and can that be proven afterwards. The provider traits keep the answer swappable — a second
//! machine is a profile in a TOML file, never a code change.

pub mod commands;
pub mod config;
pub mod fingerprint;
pub mod image;
pub mod integration;
pub mod kubernetes;
pub mod provider;
pub mod scaffold;
pub mod state;
pub mod workspace;

use crate::role::{SkillFile, SkillRef};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

const EXEC_ENVIRONMENT_SKILL: &str = include_str!("../../assets/core/skills/oat-exec-environment/SKILL.md");

/// The core skill made available to every role launched with an environment (ticket Scope).
pub fn exec_environment_skill() -> SkillRef {
    SkillRef {
        name: "oat-exec-environment".to_string(),
        files: vec![SkillFile {
            relative_path: PathBuf::from("SKILL.md"),
            contents: EXEC_ENVIRONMENT_SKILL.as_bytes().to_vec(),
            executable: false,
        }],
    }
}

/// Where the in-pod runner is installed. It is what makes a command survive a dropped exec
/// stream: the work runs detached inside the container and the stream only carries its output.
pub const RUNNER_PATH: &str = "/tmp/oat-agents/run.sh";

/// What a role dispatch needs before it can run anything.
#[derive(Debug, Clone)]
pub struct EnvSpec {
    pub env_id: String,
    pub run_id: Option<String>,
    pub role: String,
    pub worktree: PathBuf,
    pub image: image::ImageRef,
    pub user: Option<(u32, u32)>,
}

/// One command, as the caller wrote it. `argv` is never re-parsed by a shell.
#[derive(Debug, Clone)]
pub struct ExecRequest {
    pub exec_id: String,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub timeout: Option<Duration>,
    pub attach: bool,
}

#[derive(Debug, Clone)]
pub struct ExecOutcome {
    pub exit_code: i32,
    pub duration_ms: u64,
}

/// One command, as the ledger remembers it once its start and end lines have been folded back
/// together.
///
/// An entry exists from the moment a command starts, so a command that never came back is
/// still here, with `exit` unset. That is deliberate: a timeout used to leave no trace at all,
/// which made an honest agent that waited out a long suite indistinguishable from one that ran
/// nothing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub exec_id: String,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    /// The worktree the environment was bound to. This is what makes the ledgers of two roles
    /// sharing a worktree findable together: their env ids differ by role, and an earlier
    /// role's registration may be gone by the time a later one looks, but both wrote this path.
    pub worktree: PathBuf,
    pub env_id: String,
    pub role: String,
    pub image_id: String,
    pub profile: String,
    pub context: String,
    pub namespace: String,
    pub started_ms: u64,
    pub ts: String,
    /// What the code looked like when the command started, and when it came back. Both,
    /// because a command whose worktree was edited midway through verified neither version of
    /// it.
    pub tree_start: fingerprint::Fingerprint,
    pub tree_end: Option<fingerprint::Fingerprint>,
    pub exit: Option<i32>,
    pub ms: Option<u64>,
    pub timeout_secs: Option<u64>,
    /// This line closed an execution that an earlier `--attach` rejoined.
    pub attached: bool,
}

impl LedgerEntry {
    /// Whether the command ran to completion. A command still running, or one whose stream was
    /// dropped and never followed, has no result to report.
    pub fn finished(&self) -> bool {
        self.exit.is_some()
    }

    /// The tree both ends of the command agree on, if they agree at all.
    pub fn settled_tree(&self) -> Option<&fingerprint::Fingerprint> {
        let end = self.tree_end.as_ref()?;
        fingerprint::same(&self.tree_start, end).then_some(end)
    }
}

/// The durable record of an environment, written the moment it exists so a later command — or a
/// later process — can find it without asking anyone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvRecord {
    pub env_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub role: String,
    pub profile: String,
    pub worktree: PathBuf,
    pub container_path: PathBuf,
    pub image_ref: String,
    pub image_id: String,
    pub pod: String,
    pub namespace: String,
    pub context: String,
    pub created_ms: u64,
}

/// A deterministic name for one role's environment in one worktree, short enough to read in
/// `kubectl get pods` and stable across processes.
pub fn env_id(run_id: Option<&str>, role: &str, worktree: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(worktree.as_os_str().as_encoded_bytes());
    let worktree_hash = format!("{:x}", hasher.finalize());
    let run = run_id.unwrap_or("norun");
    let run = run.split('-').next_back().unwrap_or(run);
    let run: String = run
        .chars()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!(
        "oat-{}-{}-{}",
        sanitize(&run),
        sanitize(role),
        &worktree_hash[..8]
    )
}

/// Lowercase alphanumerics and dashes: what both a DNS label and a file name will accept
/// without argument.
pub fn sanitize(value: &str) -> String {
    let mapped: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = mapped.trim_matches('-').to_owned();
    if trimmed.is_empty() {
        "x".to_owned()
    } else {
        trimmed
    }
}

/// A fresh identifier per command, so a dropped stream can be reattached to the exact work it
/// was watching.
pub fn exec_id() -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(now_ms().to_string().as_bytes());
    hasher.update(std::process::id().to_string().as_bytes());
    format!("{:x}", hasher.finalize())[..12].to_owned()
}

pub fn now_ms() -> u64 {
    crate::event_log::now_ms()
}

/// The rest of the CLI is synchronous and stays that way; async lives and dies inside this
/// module.
///
/// One runtime for the process, not one per call: a Kubernetes client keeps background tasks of
/// its own, and a runtime that went away between two calls takes them with it — the client then
/// fails with nothing but "the worker closed unexpectedly" to say why.
pub fn block_on<F: Future>(future: F) -> Result<F::Output> {
    static RUNTIME: OnceLock<Option<tokio::runtime::Runtime>> = OnceLock::new();
    let runtime = RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .ok()
        })
        .as_ref()
        .ok_or_else(|| anyhow!("failed to start the async runtime for the execution environment"))?;
    Ok(runtime.block_on(future))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_environment_is_named_for_its_run_role_and_worktree() {
        let first = env_id(Some("run-2026-09-22-abc123"), "worker", Path::new("/a/one"));
        assert!(first.starts_with("oat-abc123-worker-"), "{first}");
        assert_eq!(
            first,
            env_id(Some("run-2026-09-22-abc123"), "worker", Path::new("/a/one")),
            "the same dispatch always names the same environment"
        );
        assert_ne!(
            first,
            env_id(Some("run-2026-09-22-abc123"), "reviewer", Path::new("/a/one")),
            "one role never inherits another's environment"
        );
        assert_ne!(
            first,
            env_id(Some("run-2026-09-22-abc123"), "worker", Path::new("/a/two"))
        );
        assert!(
            first.chars().all(|character| character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '-'),
            "{first} has to be a name both Kubernetes and a filesystem accept"
        );
    }

    #[test]
    fn a_name_that_cannot_be_used_is_reduced_to_one_that_can() {
        assert_eq!(sanitize("Worker/One"), "worker-one");
        assert_eq!(sanitize("--"), "x");
        assert_eq!(sanitize("oat_env.1"), "oat-env-1");
    }
}
