use crate::console;
use crate::environment::Environment;
use crate::role::Backend;
use anyhow::Result;
use clap::{Args, Subcommand};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Subcommand, Debug)]
pub enum ConsoleCommand {
    Open(OpenArgs),
    Stop(RepoArgs),
    Status(RepoArgs),
}

#[derive(Args, Debug)]
pub struct OpenArgs {
    #[arg(long)]
    pub repo: PathBuf,
    #[arg(long, default_value = "claude")]
    pub agent: String,
}

#[derive(Args, Debug)]
pub struct RepoArgs {
    #[arg(long)]
    pub repo: PathBuf,
}

pub fn run(command: ConsoleCommand, env: &dyn Environment) -> Result<Value> {
    match command {
        ConsoleCommand::Open(args) => open(args, env),
        ConsoleCommand::Stop(args) => stop(args, env),
        ConsoleCommand::Status(args) => status(args, env),
    }
}

fn open(args: OpenArgs, env: &dyn Environment) -> Result<Value> {
    let backend = Backend::from_str(&args.agent)?;
    let record = console::open(env, &args.repo, backend)?;
    Ok(json!({"repo": record.repo, "backend": record.backend, "session": record.session}))
}

fn stop(args: RepoArgs, env: &dyn Environment) -> Result<Value> {
    let record = console::stop(env, &args.repo)?;
    Ok(json!({"repo": record.repo, "session": record.session, "stopped": true}))
}

fn status(args: RepoArgs, env: &dyn Environment) -> Result<Value> {
    match console::status(env, &args.repo)? {
        Some((record, alive)) => Ok(json!({
            "repo": record.repo,
            "backend": record.backend,
            "session": record.session,
            "alive": alive,
        })),
        None => Ok(json!({"repo": args.repo.to_string_lossy(), "alive": false, "opened": false})),
    }
}
