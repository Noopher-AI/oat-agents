//! An index of what every Dispatch in a Run has spent, read from its transcript. This lives
//! here rather than under a live-view module (which is F3's) so the lifecycle this ticket
//! ships does not depend on a module it must not create (ticket §3.1); F3's live view is
//! expected to consume this module rather than own it.

use crate::environment::Environment;
use crate::pricing::{Tokens, Usage};
use crate::store::{DispatchRecord, Store};
use anyhow::Result;
use serde_json::Value;
use std::fs;
use std::path::Path;

pub(crate) fn usage_from_transcript(path: &Path) -> Usage {
    let mut usage = Usage::default();
    let Ok(contents) = fs::read_to_string(path) else {
        return usage;
    };
    for line in contents.lines() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(message) = entry.get("message") else {
            continue;
        };
        let (Some(model), Some(block)) = (
            message.get("model").and_then(Value::as_str),
            message.get("usage"),
        ) else {
            continue;
        };
        let count = |key: &str| block.get(key).and_then(Value::as_u64).unwrap_or(0);
        usage.add(
            model,
            &Tokens {
                input: count("input_tokens"),
                output: count("output_tokens"),
                cache_write_5m: count("cache_creation_input_tokens"),
                cache_write_1h: 0,
                cache_read: count("cache_read_input_tokens"),
            },
        );
    }
    usage
}

/// Every Dispatch's spend in one Run, keyed by Dispatch id.
#[derive(Debug, Default)]
pub struct UsageIndex {
    pub by_dispatch: Vec<(String, Usage)>,
}

impl UsageIndex {
    pub fn total(&self) -> Usage {
        let mut total = Usage::default();
        for (_, usage) in &self.by_dispatch {
            for (model, tokens) in &usage.by_model {
                total.add(model, tokens);
            }
        }
        total
    }
}

pub fn build_for_run(env: &dyn Environment, store: &Store, run_id: &str) -> Result<UsageIndex> {
    let mut index = UsageIndex::default();
    let dispatches_dir = store.root().join(run_id).join("dispatches");
    if !dispatches_dir.exists() {
        return Ok(index);
    }
    for entry in fs::read_dir(&dispatches_dir)? {
        let entry = entry?;
        let dispatch_id = entry.file_name().to_string_lossy().to_string();
        let Ok(dispatch) = store.load_dispatch(run_id, &dispatch_id) else {
            continue;
        };
        if let Some(usage) = usage_for_dispatch(env, &dispatch) {
            index.by_dispatch.push((dispatch_id, usage));
        }
    }
    Ok(index)
}

fn usage_for_dispatch(env: &dyn Environment, dispatch: &DispatchRecord) -> Option<Usage> {
    if matches!(dispatch.backend.parse(), Ok(crate::role::Backend::Codex)) {
        let root = crate::codex_session::codex_sessions_dir(env)?;
        let paths = crate::codex_session::Locator::default().find(&root, &dispatch.worktree);
        return (!paths.is_empty()).then(|| crate::codex_session::session(&paths).usage);
    }
    let projects_root = crate::transcript::claude_projects_dir(env)?;
    let newest = crate::transcript::find_transcript(&projects_root, &dispatch.backend, &dispatch.worktree)?;
    Some(usage_from_transcript(&newest))
}
