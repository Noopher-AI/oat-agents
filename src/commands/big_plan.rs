use crate::environment::Environment;
use crate::error::{codes, err};
use crate::event_log::EventLog;
use crate::store::Store;
use anyhow::Result;
use clap::{Args, Subcommand};
use serde_json::{json, Value};

#[derive(Subcommand, Debug)]
pub enum BigPlanCommand {
    Show(ShowArgs),
    List(ListArgs),
}

#[derive(Args, Debug)]
pub struct ShowArgs {
    #[arg(long)]
    pub run: Option<String>,
}

#[derive(Args, Debug)]
pub struct ListArgs;

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

pub fn run(command: BigPlanCommand, env: &dyn Environment) -> Result<Value> {
    match command {
        BigPlanCommand::Show(args) => show(args, env),
        BigPlanCommand::List(args) => list(args, env),
    }
}

fn big_plan_for(env: &dyn Environment, run_id: &str) -> Result<String> {
    let log = EventLog::open(env);
    for entry in log.read_run(run_id)? {
        if entry.event == crate::event_log::events::RUN_CREATED {
            if let Some(details) = entry.details {
                if let Some(text) = details.get("big_plan").and_then(|v| v.as_str()) {
                    return Ok(text.to_string());
                }
            }
        }
    }
    Err(err(codes::INVALID_INPUT, format!("no big plan recorded for run '{run_id}'")))
}

fn show(args: ShowArgs, env: &dyn Environment) -> Result<Value> {
    let store = Store::open(env)?;
    let run_id = resolve_run(&args.run, env, &store)?;
    let big_plan = big_plan_for(env, &run_id)?;
    Ok(json!({"run_id": run_id, "big_plan": big_plan}))
}

fn list(_args: ListArgs, env: &dyn Environment) -> Result<Value> {
    let store = Store::open(env)?;
    let mut runs = Vec::new();
    for id in store.list_run_ids()? {
        let big_plan = big_plan_for(env, &id).unwrap_or_default();
        runs.push(json!({"run_id": id, "big_plan": big_plan}));
    }
    Ok(json!({"runs": runs}))
}
