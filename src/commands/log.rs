// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

use crate::environment::Environment;
use crate::error::{codes, err};
use crate::event_log::{now_iso, EventLog, LogEntry};
use crate::store::Store;
use anyhow::Result;
use clap::{Args, Subcommand};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Subcommand, Debug)]
pub enum LogCommand {
    /// One Run's workflow log.
    Show(ShowArgs),
    /// Append a decision, a needs-human question or another event to the workflow log.
    Record(RecordArgs),
    /// Every agent that appears in a Run's workflow log.
    Agents(AgentsArgs),
    /// Every Run and whether it is still open.
    Runs(RunsArgs),
}

#[derive(Args, Debug)]
pub struct ShowArgs {
    #[arg(long)]
    pub run: Option<String>,
    #[arg(long)]
    pub agent: Option<String>,
    #[arg(long)]
    pub event: Option<String>,
    #[arg(long)]
    pub since: Option<String>,
    #[arg(long, default_value_t = 200)]
    pub limit: usize,
    #[arg(long, default_value = "json")]
    pub format: String,
}

#[derive(Args, Debug)]
pub struct RecordArgs {
    #[arg(long)]
    pub run: Option<String>,
    #[arg(long)]
    pub dispatch: Option<String>,
    #[arg(long)]
    pub agent: Option<String>,
    #[arg(long)]
    pub event: String,
    #[arg(long)]
    pub message: Option<String>,
}

#[derive(Args, Debug)]
pub struct AgentsArgs {
    #[arg(long)]
    pub run: Option<String>,
}

#[derive(Args, Debug)]
pub struct RunsArgs {
    /// Restricts the listing to Runs whose repository matches this path (a console's own
    /// read-only view, ADR-0005: it observes only its repository's Runs).
    #[arg(long)]
    pub repo: Option<std::path::PathBuf>,
}

fn resolve_run(run: &Option<String>, env: &dyn Environment, store: &Store) -> Result<String> {
    if let Some(r) = run {
        return Ok(r.clone());
    }
    if let Some(r) = env.var("OAT_RUN_ID") {
        return Ok(r);
    }
    store
        .most_recently_updated_run()?
        .ok_or_else(|| err(codes::RUN_NOT_BOUND, "no run bound and none found; pass --run"))
}

pub fn run(command: LogCommand, env: &dyn Environment) -> Result<Value> {
    match command {
        LogCommand::Show(args) => show(args, env),
        LogCommand::Record(args) => record(args, env),
        LogCommand::Agents(args) => agents(args, env),
        LogCommand::Runs(args) => runs(args, env),
    }
}

const RECORD_EVENTS: &[&str] = &[
    "decision",
    "delegation",
    "review-result",
    "retry",
    "blocker",
    "note",
    "agent-exit",
    "needs-human",
    "answered",
];

fn record(args: RecordArgs, env: &dyn Environment) -> Result<Value> {
    if !RECORD_EVENTS.contains(&args.event.as_str()) {
        return Err(err(
            codes::INVALID_CLI_ARGUMENTS,
            format!(
                "--event must be one of: {}; got '{}'",
                RECORD_EVENTS.join(", "),
                args.event
            ),
        ));
    }
    let store = Store::open(env)?;
    let run_id = resolve_run(&args.run, env, &store)?;
    let log = EventLog::open(env);
    let event_name = match args.event.as_str() {
        "needs-human" => crate::event_log::events::NEEDS_HUMAN.to_string(),
        "answered" => crate::event_log::events::HUMAN_ANSWER.to_string(),
        "agent-exit" => crate::event_log::events::AGENT_EXIT.to_string(),
        other => other.to_string(),
    };
    log.record(&LogEntry {
        timestamp: now_iso(),
        run_id: run_id.clone(),
        dispatch_id: args.dispatch.clone(),
        agent: args.agent.clone(),
        event: event_name,
        details: args.message.map(|m| json!({"message": m})),
    })?;
    Ok(json!({"run_id": run_id, "event": args.event}))
}

fn show(args: ShowArgs, env: &dyn Environment) -> Result<Value> {
    let store = Store::open(env)?;
    let run_id = resolve_run(&args.run, env, &store)?;
    let log = EventLog::open(env);
    let mut entries = log.read_run(&run_id)?;

    if let Some(agent) = &args.agent {
        entries.retain(|e| e.agent.as_deref() == Some(agent.as_str()));
    }
    if let Some(event) = &args.event {
        entries.retain(|e| &e.event == event);
    }
    if let Some(since) = &args.since {
        entries.retain(|e| e.timestamp.as_str() >= since.as_str());
    }
    if entries.len() > args.limit {
        let start = entries.len() - args.limit;
        entries = entries[start..].to_vec();
    }

    if args.format == "text" {
        let lines: Vec<String> = entries
            .iter()
            .map(|e| format!("{} {} {}", e.timestamp, e.event, e.agent.clone().unwrap_or_default()))
            .collect();
        Ok(json!({"run_id": run_id, "text": lines.join("\n")}))
    } else {
        Ok(json!({"run_id": run_id, "entries": entries}))
    }
}

fn agents(args: AgentsArgs, env: &dyn Environment) -> Result<Value> {
    let store = Store::open(env)?;
    let run_id = resolve_run(&args.run, env, &store)?;
    let log = EventLog::open(env);
    let entries = log.read_run(&run_id)?;
    let agents: BTreeSet<String> = entries.into_iter().filter_map(|e| e.agent).collect();
    Ok(json!({"run_id": run_id, "agents": agents}))
}

fn runs(args: RunsArgs, env: &dyn Environment) -> Result<Value> {
    let store = Store::open(env)?;
    let repo_filter = args
        .repo
        .as_ref()
        .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()));
    let mut runs = Vec::new();
    for id in store.list_run_ids()? {
        let record = store.load_run(&id)?;
        if let Some(repo) = &repo_filter {
            let run_repo = std::path::PathBuf::from(&record.repo);
            let run_repo = run_repo.canonicalize().unwrap_or(run_repo);
            if &run_repo != repo {
                continue;
            }
        }
        runs.push(json!({"run_id": id, "open": record.is_open()}));
    }
    Ok(json!({"runs": runs}))
}
