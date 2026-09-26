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

fn usage_from_transcript(path: &Path) -> Usage {
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
    let home = env.home_dir()?;
    let projects_root = home.join(".claude").join("projects");
    let slug: String = dispatch
        .worktree
        .chars()
        .map(|c| if matches!(c, '/' | '.') { '-' } else { c })
        .collect();
    let project = projects_root.join(slug);
    let newest = fs::read_dir(&project)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
        .max_by_key(|p| fs::metadata(p).and_then(|m| m.modified()).ok())?;
    Some(usage_from_transcript(&newest))
}
