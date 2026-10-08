// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

//! The liveness block a timed-out `run wait` carries (ticket §4.4): the only moment a
//! meta-agent has to decide whether to keep waiting, so that is where the evidence goes.

use crate::environment::Environment;
use crate::store::Store;
use crate::transcript;
use anyhow::Result;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Per still-active Dispatch: `working` / `waiting` / `stalled`, how long it has been silent,
/// the outstanding tool where there is one, and whether the silence has outlasted the kind of
/// work it is doing. A Codex-backed Dispatch has no transcript this crate can read
/// (`transcript`'s module doc), so it is reported with no pulse rather than a guessed one.
pub fn report_for_run(env: &dyn Environment, store: &Store, run_id: &str) -> Result<Value> {
    let dispatches_dir = store.root().join(run_id).join("dispatches");
    let mut agents = Vec::new();
    if dispatches_dir.exists() {
        for entry in std::fs::read_dir(&dispatches_dir)? {
            let entry = entry?;
            let dispatch_id = entry.file_name().to_string_lossy().to_string();
            let Ok(dispatch) = store.load_dispatch(run_id, &dispatch_id) else {
                continue;
            };
            if dispatch.released_at.is_some() || dispatch.is_settled() {
                continue;
            }
            let now = now_ms();
            let entry = match transcript::pulse_for(env, &dispatch) {
                Some(pulse) => json!({
                    "dispatch_id": dispatch_id,
                    "role": dispatch.role,
                    "liveness": pulse.liveness(now, transcript::QUIET_AFTER_MS).as_str(),
                    "silent_ms": pulse.silent_ms(now),
                    "outstanding": pulse.outstanding(),
                    "overrunning": pulse.overrunning(now, transcript::QUIET_AFTER_MS, transcript::OVERRUN_AFTER_MS),
                }),
                None => json!({
                    "dispatch_id": dispatch_id,
                    "role": dispatch.role,
                    "liveness": Value::Null,
                    "note": "no transcript available for this backend",
                }),
            };
            agents.push(entry);
        }
    }
    Ok(json!({"agents": agents}))
}
