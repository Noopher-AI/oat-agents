//! The live view's reading of a Run: its Dispatches from the Store and its history from the
//! workflow log, folded into the rows every screen draws — one per Run in the picker, one per
//! Dispatch in the roster, one per log entry in the timeline. Only files an observer may
//! always read are read here; nothing touches a session.

use crate::checklist::{ChecklistStore, ItemProgress};
use crate::event_log::{events, EventLog, LogEntry};
use crate::role::CoreRole;
use crate::store::{DispatchRecord, RunRecord, Settlement, Store};
use anyhow::Result;
use serde_json::{Map, Value};
use std::collections::HashMap;

/// A text field of a timeline row, `None` when it is absent or empty.
pub fn string_field(row: &Value, key: &str) -> Option<String> {
    row.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|value| !value.is_empty())
}

/// Renders a recorded UTC timestamp in the operator's own time zone. The log stays UTC on
/// disk; only what a person reads is converted.
pub fn local_time(epoch_ms: Option<u64>, rfc3339: Option<&str>, format: &str) -> Option<String> {
    let instant = match (epoch_ms, rfc3339) {
        (Some(millis), _) => chrono::DateTime::from_timestamp_millis(millis as i64)?.fixed_offset(),
        (None, Some(text)) => chrono::DateTime::parse_from_rfc3339(text).ok()?,
        (None, None) => return None,
    };
    Some(instant.with_timezone(&chrono::Local).format(format).to_string())
}

pub fn epoch_seconds(rfc3339: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .ok()
        .map(|stamp| stamp.timestamp())
}

/// A duration in the units a person would say out loud: `45s`, `12m30s`, `9h24m`, `2d3h`.
pub fn human_span(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let (days, hours, minutes, rest) = (
        seconds / 86_400,
        (seconds % 86_400) / 3_600,
        (seconds % 3_600) / 60,
        seconds % 60,
    );
    if days > 0 {
        format!("{days}d{hours}h")
    } else if hours > 0 {
        format!("{hours}h{minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m{rest:02}s")
    } else {
        format!("{rest}s")
    }
}

/// The operator's current UTC offset, for saying which clock a screen shows.
pub fn local_offset() -> String {
    chrono::Local::now().format("%:z").to_string()
}

/// One Dispatch as the roster shows it.
#[derive(Clone, Debug, Default)]
pub struct AgentRow {
    pub hash_id: String,
    pub role: Option<String>,
    /// The backend its session runs.
    pub agent: Option<String>,
    pub dispatch_id: Option<String>,
    /// The worktree's path.
    pub worktree: Option<String>,
    /// The Dispatch's tmux session, which is also where input reaches it.
    pub terminal_handle: Option<String>,
    pub branch: Option<String>,
    /// What its worktree's diff is taken against.
    pub base: Option<String>,
    pub entered: Option<String>,
    pub exited: Option<String>,
    pub outcome: Option<String>,
    /// The execution environment its commands ran in, when it had one.
    pub env_id: Option<String>,
    pub image_id: Option<String>,
    /// Why a role that asked for an execution environment ran on the host instead
    /// (`.dev_docs/CONTEXT.md`, *Execution environment*: reported, never left to be inferred).
    pub env_skipped: Option<String>,
}

impl AgentRow {
    pub fn state(&self) -> &'static str {
        if self.exited.is_some() {
            "exited"
        } else {
            "active"
        }
    }

    /// How long it has been at it: up to its exit, or up to now while it runs.
    pub fn seconds(&self) -> Option<i64> {
        let started = epoch_seconds(self.entered.as_deref()?)?;
        let end = match self.exited.as_deref() {
            Some(exited) => epoch_seconds(exited)?,
            None => chrono::Utc::now().timestamp(),
        };
        Some((end - started).max(0))
    }

    /// The coordinator, the one Dispatch that asks a person anything.
    pub fn is_meta(&self) -> bool {
        matches!(self.role.as_deref(), Some(role) if role == CoreRole::Meta.name() || role == "meta")
    }

    /// `pod:<env>`, `HOST (no pod)` for a role that asked for a pod and did not get one, or
    /// empty for a role that never asked.
    pub fn exec(&self) -> String {
        match (&self.env_id, &self.env_skipped) {
            (Some(env_id), _) => format!("pod:{env_id}"),
            (None, Some(_)) => "HOST (no pod)".to_string(),
            (None, None) => String::new(),
        }
    }
}

/// One Run as the picker lists it.
#[derive(Clone, Debug, Default)]
pub struct RunSummary {
    pub run_id: String,
    pub repo: Option<String>,
    pub backend: String,
    /// First line of the Big Plan, as a title.
    pub objective: Option<String>,
    /// What the Run is waiting on a person to answer, if anything.
    pub question: Option<String>,
    pub started: String,
    pub last: String,
    pub agents: usize,
    /// Dispatches with no exit yet.
    pub active: usize,
    pub open: bool,
    /// `pod:<profile>`, `host`, or empty for a Run older than the exec events.
    pub exec: String,
}

impl RunSummary {
    /// A Run runs until its coordinator finishes it.
    pub fn running(&self) -> bool {
        self.open
    }

    /// How long the Run has taken: still counting while it runs, first to last event once
    /// it has stopped.
    pub fn seconds(&self) -> Option<i64> {
        let started = epoch_seconds(&self.started)?;
        let end = if self.running() {
            chrono::Utc::now().timestamp()
        } else {
            epoch_seconds(&self.last)?
        };
        Some((end - started).max(0))
    }

    /// The folder name, which is all that fits in a list.
    pub fn repo_name(&self) -> Option<&str> {
        self.repo
            .as_deref()
            .and_then(|repo| repo.trim_end_matches('/').rsplit('/').next())
            .filter(|name| !name.is_empty())
    }
}

/// Everything the view reads about one Run in one pass.
#[derive(Clone, Debug)]
pub struct RunSnapshot {
    pub run: RunRecord,
    pub dispatches: Vec<DispatchRecord>,
    /// The workflow log, one row per entry, its details flattened beside `time`, `event`,
    /// `dispatch_id` and the Dispatch's `hash_id`.
    pub events: Vec<Value>,
}

impl RunSnapshot {
    pub fn read(store: &Store, log: &EventLog, run_id: &str) -> Result<Self> {
        let run = store.load_run(run_id)?;
        let dispatches = store.list_dispatches(run_id);
        let hashes: HashMap<String, String> = dispatches
            .iter()
            .map(|dispatch| (dispatch.id.clone(), dispatch_hash(&run, dispatch)))
            .collect();
        let events = log
            .read_run(run_id)
            .unwrap_or_default()
            .iter()
            .map(|entry| event_row(entry, &hashes))
            .collect();
        Ok(Self {
            run,
            dispatches,
            events,
        })
    }

    pub fn agents(&self) -> Vec<AgentRow> {
        agent_rows(&self.run, &self.dispatches, &self.events)
    }

    pub fn summary(&self) -> RunSummary {
        let agents = self.agents();
        let last = self
            .events
            .iter()
            .filter_map(|row| string_field(row, "time"))
            .max()
            .unwrap_or_else(|| self.run.created_at.clone());
        RunSummary {
            run_id: self.run.id.clone(),
            repo: Some(self.run.repo.clone()).filter(|repo| !repo.is_empty()),
            backend: self.run.backend.clone(),
            objective: self
                .run
                .big_plan
                .as_deref()
                .and_then(|plan| plan.lines().map(str::trim).find(|line| !line.is_empty()))
                .map(|line| line.trim_start_matches('#').trim().to_owned()),
            question: open_question(&self.events),
            started: self.run.created_at.clone(),
            last: self.run.closed_at.clone().unwrap_or(last),
            agents: agents.len(),
            active: agents.iter().filter(|agent| agent.state() == "active").count(),
            open: self.run.is_open(),
            exec: run_exec(&self.events),
        }
    }
}

/// Every Run the Store holds, read for the picker.
pub fn run_summaries(store: &Store, log: &EventLog) -> Vec<RunSummary> {
    store
        .list_run_ids()
        .unwrap_or_default()
        .iter()
        .filter_map(|run_id| RunSnapshot::read(store, log, run_id).ok())
        .map(|snapshot| snapshot.summary())
        .collect()
}

/// The Dispatch's hash_id, derived the way the launch derived it: the coordinator's under
/// `meta`, every other role's under its own name.
pub fn dispatch_hash(run: &RunRecord, dispatch: &DispatchRecord) -> String {
    dispatch.hash_id(run)
}

fn event_row(entry: &LogEntry, hashes: &HashMap<String, String>) -> Value {
    let mut row = match &entry.details {
        Some(Value::Object(details)) => details.clone(),
        _ => Map::new(),
    };
    row.insert("time".into(), Value::String(entry.timestamp.clone()));
    row.insert("event".into(), Value::String(entry.event.clone()));
    if let Some(dispatch_id) = &entry.dispatch_id {
        row.insert("dispatch_id".into(), Value::String(dispatch_id.clone()));
        if let Some(hash) = hashes.get(dispatch_id) {
            row.insert("hash_id".into(), Value::String(hash.clone()));
        }
    }
    if let Some(agent) = &entry.agent {
        row.insert("role".into(), Value::String(agent.clone()));
    }
    Value::Object(row)
}

/// One row per Dispatch, in the order they were launched. The Store says who each one is;
/// the log says when it entered and how it ended.
pub fn agent_rows(run: &RunRecord, dispatches: &[DispatchRecord], events: &[Value]) -> Vec<AgentRow> {
    dispatches
        .iter()
        .map(|dispatch| {
            let own = |event: &'static str| {
                events.iter().filter(move |row| {
                    string_field(row, "event").as_deref() == Some(event)
                        && string_field(row, "dispatch_id").as_deref() == Some(dispatch.id.as_str())
                })
            };
            let hash_id = dispatch_hash(run, dispatch);
            let exit = own(events::AGENT_EXIT).next();
            // Settlement is the exit that matters; a release or the Run closing only ends a
            // Dispatch that never settled.
            let exited = exit
                .and_then(|row| string_field(row, "time"))
                .or_else(|| dispatch.released_at.clone())
                .or_else(|| run.closed_at.clone());
            let outcome = exit
                .and_then(|row| string_field(row, "outcome"))
                .or_else(|| match dispatch.settled {
                    Some(Settlement::Succeeded) => Some("succeeded".to_owned()),
                    Some(Settlement::Failed) => Some("failed".to_owned()),
                    None => None,
                })
                .filter(|_| exited.is_some());
            AgentRow {
                terminal_handle: Some(crate::session::tmux::Tmux::session_name(&hash_id)),
                hash_id,
                role: Some(dispatch.role.clone()),
                agent: Some(dispatch.backend.clone()).filter(|backend| !backend.is_empty()),
                dispatch_id: Some(dispatch.id.clone()),
                worktree: Some(dispatch.worktree.clone()).filter(|path| !path.is_empty()),
                branch: Some(dispatch.branch.clone()).filter(|branch| !branch.is_empty()),
                base: Some(run.base_branch.clone()).filter(|base| !base.is_empty()),
                entered: own(events::AGENT_ENTER)
                    .next()
                    .and_then(|row| string_field(row, "time"))
                    .or_else(|| Some(dispatch.created_at.clone())),
                exited,
                outcome,
                env_id: dispatch.env_id.clone(),
                image_id: dispatch.image_id.clone(),
                env_skipped: dispatch.env_skipped.clone(),
            }
        })
        .collect()
}

/// The question a Run is waiting on a person to answer: a `needs_human` entry with no later
/// answer. Closing the Run retires it too — nobody is left to read the answer.
pub fn open_question(events: &[Value]) -> Option<String> {
    let mut question = None;
    for row in events {
        match string_field(row, "event").as_deref() {
            Some(events::NEEDS_HUMAN) => {
                question = Some(
                    string_field(row, "message").unwrap_or_else(|| "no question recorded".to_owned()),
                )
            }
            Some(events::HUMAN_ANSWER) | Some(events::RUN_CLOSED) => question = None,
            _ => {}
        }
    }
    question
}

/// Where a Run's roles run their commands, from the log's exec events.
pub fn run_exec(events: &[Value]) -> String {
    events
        .iter()
        .rev()
        .find_map(|row| match string_field(row, "event").as_deref() {
            Some(events::EXEC_PROFILE_SELECTED) => {
                string_field(row, "profile").map(|profile| format!("pod:{profile}"))
            }
            Some(events::EXEC_PROFILE_NONE) => Some("host".to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

/// One line of the checklist overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecklistRow {
    pub done: bool,
    pub text: String,
    /// Where the work behind a linked item stands; `None` for an unlinked item.
    pub progress: Option<ItemProgress>,
}

/// The open Run's checklist, as the overlay shows it. Without the Run store, linked items
/// show no progress.
pub fn checklist_items(
    checklists: Option<&ChecklistStore>,
    store: Option<&Store>,
    run_id: &str,
) -> Option<Vec<ChecklistRow>> {
    let checklist = checklists?.load(run_id).ok()?;
    let progress = match store {
        Some(store) => crate::checklist::progress(store, run_id, &checklist),
        None => Vec::new(),
    };
    Some(
        checklist
            .items
            .into_iter()
            .zip(checklist.checked.into_iter().chain(std::iter::repeat(false)))
            .zip(progress.into_iter().chain(std::iter::repeat(None)))
            .map(|((text, done), progress)| ChecklistRow { done, text, progress })
            .collect(),
    )
}
