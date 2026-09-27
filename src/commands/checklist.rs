// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

use crate::checklist::{self, ChecklistStore};
use crate::environment::Environment;
use crate::error::{codes, err};
use crate::event_log::{events, now_iso, EventLog, LogEntry};
use crate::store::Store;
use anyhow::Result;
use clap::{Args, Subcommand};
use serde_json::{json, Value};

#[derive(Subcommand, Debug)]
pub enum ChecklistCommand {
    Update(UpdateArgs),
    Show(ShowArgs),
}

#[derive(Args, Debug)]
pub struct ShowArgs {
    #[arg(long)]
    pub run: Option<String>,
}

#[derive(Args, Debug)]
pub struct UpdateArgs {
    #[arg(long)]
    pub run: Option<String>,
    #[arg(long)]
    pub items: Vec<String>,
    /// `N=<name>`: item N tracks the Dispatches fired with `--name <name>` or
    /// `--name <name>-<anything>`.
    #[arg(long)]
    pub link: Vec<String>,
    #[arg(long)]
    pub unlink: Vec<usize>,
    #[arg(long)]
    pub check: Vec<usize>,
    #[arg(long)]
    pub uncheck: Vec<usize>,
}

pub fn run(command: ChecklistCommand, env: &dyn Environment) -> Result<Value> {
    match command {
        ChecklistCommand::Update(args) => update(args, env),
        ChecklistCommand::Show(args) => show(args, env),
    }
}

/// A read-only view of the current checklist, for the live view's checklist panel and the
/// console's read-only accessors — neither should have to go through `update` to read state.
fn show(args: ShowArgs, env: &dyn Environment) -> Result<Value> {
    let run_id = bound_run(&args.run, env)?;
    let store = ChecklistStore::open(env);
    let checklist = store.load(&run_id)?;
    Ok(json!({
        "run_id": run_id,
        "progress": progress(&checklist, &run_id, env),
        "checklist": checklist,
    }))
}

/// Each linked item's progress, as the live view's panel shows it; `null` for an unlinked one.
fn progress(checklist: &checklist::Checklist, run_id: &str, env: &dyn Environment) -> Value {
    match Store::open(env) {
        Ok(store) => json!(checklist::progress(&store, run_id, checklist)),
        Err(_) => json!(vec![Value::Null; checklist.items.len()]),
    }
}

fn parse_link(arg: &str) -> Result<(usize, String)> {
    arg.split_once('=')
        .and_then(|(n, name)| Some((n.trim().parse().ok()?, name.trim().to_owned())))
        .filter(|(_, name)| !name.is_empty())
        .ok_or_else(|| err(codes::INVALID_INPUT, format!("--link takes N=<name>, got {arg:?}")))
}

fn bound_run(run: &Option<String>, env: &dyn Environment) -> Result<String> {
    run.clone()
        .or_else(|| env.var("OAT_RUN_ID"))
        .ok_or_else(|| err(codes::RUN_NOT_BOUND, "no run bound; pass --run or set OAT_RUN_ID"))
}

fn update(args: UpdateArgs, env: &dyn Environment) -> Result<Value> {
    let run_id = bound_run(&args.run, env)?;
    // Parsed before anything is saved, so a malformed link changes nothing.
    let links = args
        .link
        .iter()
        .map(|arg| parse_link(arg))
        .collect::<Result<Vec<_>>>()?;
    let store = ChecklistStore::open(env);
    let log = EventLog::open(env);

    let mut checklist = if !args.items.is_empty() {
        store.replace_items(&run_id, args.items.clone())?
    } else {
        store.load(&run_id)?
    };

    for (n, name) in links {
        checklist = store.set_link(&run_id, n, Some(name))?;
    }
    for n in &args.unlink {
        checklist = store.set_link(&run_id, *n, None)?;
    }
    for n in &args.check {
        checklist = store.set_checked(&run_id, *n, true)?;
    }
    for n in &args.uncheck {
        checklist = store.set_checked(&run_id, *n, false)?;
    }

    log.record(&LogEntry {
        timestamp: now_iso(),
        run_id: run_id.clone(),
        dispatch_id: None,
        agent: None,
        event: events::CHECKLIST_UPDATED.to_string(),
        details: Some(json!({"items": checklist.items.len()})),
    })?;

    Ok(json!({"run_id": run_id, "checklist": checklist}))
}
