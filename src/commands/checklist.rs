use crate::checklist::ChecklistStore;
use crate::environment::Environment;
use crate::error::{codes, err};
use crate::event_log::{events, now_iso, EventLog, LogEntry};
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
    Ok(json!({"run_id": run_id, "checklist": checklist}))
}

fn bound_run(run: &Option<String>, env: &dyn Environment) -> Result<String> {
    run.clone()
        .or_else(|| env.var("OAT_RUN_ID"))
        .ok_or_else(|| err(codes::RUN_NOT_BOUND, "no run bound; pass --run or set OAT_RUN_ID"))
}

fn update(args: UpdateArgs, env: &dyn Environment) -> Result<Value> {
    let run_id = bound_run(&args.run, env)?;
    let store = ChecklistStore::open(env);
    let log = EventLog::open(env);

    let mut checklist = if !args.items.is_empty() {
        store.replace_items(&run_id, args.items.clone())?
    } else {
        store.load(&run_id)?
    };

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
