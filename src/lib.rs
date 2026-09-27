// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

pub mod checklist;
pub mod codex_session;
pub mod commands;
pub mod concurrency;
pub mod console;
pub mod env;
pub mod environment;
pub mod error;
pub mod event_log;
pub mod hashing;
pub mod launch;
pub mod liveness;
pub mod plugin;
pub mod plugins;
pub mod pricing;
pub mod role;
pub mod session;
pub mod store;
pub mod transcript;
pub mod trust;
pub mod tui;
pub mod usage;
pub mod worktree;

use clap::{Parser, Subcommand};
use env::integration::ExecEnvironments;
use environment::Environment;
use error::{codes, err};
use role::RoleCatalog;
use serde_json::Value;

#[derive(Parser, Debug)]
#[command(name = "oat-agents", version, about = "Runs a supervised team of coding agents on one machine")]
pub struct Cli {
    #[command(subcommand)]
    pub command: TopCommand,
}

#[derive(Subcommand, Debug)]
pub enum TopCommand {
    /// The coordinator's own lifecycle.
    Meta {
        #[command(subcommand)]
        command: commands::meta::MetaCommand,
    },
    /// Launching a non-core role.
    Role {
        #[command(subcommand)]
        command: commands::role::RoleCommand,
    },
    /// Run-level lifecycle: the Run inbox and cleanup.
    Run {
        #[command(subcommand)]
        command: commands::run::RunCommand,
    },
    /// One Dispatch's settlement and observation.
    Dispatch {
        #[command(subcommand)]
        command: commands::dispatch::DispatchCommand,
    },
    /// The workflow log.
    Log {
        #[command(subcommand)]
        command: commands::log::LogCommand,
    },
    /// The Big Plan a Run was fired with.
    BigPlan {
        #[command(subcommand)]
        command: commands::big_plan::BigPlanCommand,
    },
    /// The Run's checklist.
    Checklist {
        #[command(subcommand)]
        command: commands::checklist::ChecklistCommand,
    },
    /// Execution environments: containers a Dispatch's build and test commands run in.
    Env {
        #[command(subcommand)]
        command: env::commands::EnvSubcommand,
    },
    /// A repository's `oat-console` (ADR-0005).
    Console {
        #[command(subcommand)]
        command: commands::console::ConsoleCommand,
    },
    /// The live view: every Run, every agent's live terminal, its log and diff, and the
    /// checklist.
    Tui,
    /// Declares a repository's pinned plugins and, optionally, its execution profile.
    Init(commands::init::InitArgs),
    /// Trusting and inspecting plugin sources on this machine.
    Plugin {
        #[command(subcommand)]
        command: commands::plugin::PluginCommand,
    },
}

/// Exactly one of `--prompt` or `--input-file` is required across several commands; this
/// resolves that pair once instead of repeating the check.
pub fn resolve_text_input(prompt: &Option<String>, input_file: &Option<std::path::PathBuf>) -> anyhow::Result<String> {
    match (prompt, input_file) {
        (Some(p), None) => Ok(p.clone()),
        (None, Some(path)) => std::fs::read_to_string(path)
            .map_err(|e| err(codes::INVALID_INPUT, format!("could not read {}: {e}", path.display()))),
        (Some(_), Some(_)) => Err(err(
            codes::INVALID_CLI_ARGUMENTS,
            "pass exactly one of --prompt or --input-file",
        )),
        (None, None) => Err(err(
            codes::INVALID_CLI_ARGUMENTS,
            "pass exactly one of --prompt or --input-file",
        )),
    }
}

/// The one testable entry point: every command's logic runs through here against an injected
/// `Environment` and `RoleCatalog`, so a test never needs a real model, a real home directory,
/// or a real plugin source. Execution environments go through their own seam
/// (`env::integration::ExecEnvironments`) for the same reason: a test of `role fire`'s wiring
/// should not need a cluster.
pub fn execute(cli: Cli, env: &dyn Environment, catalog: &dyn RoleCatalog) -> anyhow::Result<Value> {
    execute_with_exec(cli, env, catalog, &env::integration::LiveExecEnvironments)
}

pub fn execute_with_exec(
    cli: Cli,
    env: &dyn Environment,
    _catalog: &dyn RoleCatalog,
    exec: &dyn ExecEnvironments,
) -> anyhow::Result<Value> {
    // meta fire, console open, role fire and tui each build their own real catalog now — from
    // `.oat/plugins.toml` and its snapshot (F5). `_catalog` stays part of the signature so
    // existing callers (and every test built against it) do not have to change.
    match cli.command {
        TopCommand::Meta { command } => commands::meta::run(command, env, exec),
        TopCommand::Role { command } => commands::role::run(command, env, exec),
        TopCommand::Run { command } => commands::run::run(command, env, exec),
        TopCommand::Dispatch { command } => commands::dispatch::run(command, env, exec),
        TopCommand::Log { command } => commands::log::run(command, env),
        TopCommand::BigPlan { command } => commands::big_plan::run(command, env),
        TopCommand::Checklist { command } => commands::checklist::run(command, env),
        TopCommand::Env { command } => {
            let log = event_log::EventLog::open(env);
            env::commands::execute(env, &log, command)
        }
        TopCommand::Console { command } => commands::console::run(command, env),
        TopCommand::Tui => commands::tui::run(env),
        TopCommand::Init(args) => commands::init::run(args, env),
        TopCommand::Plugin { command } => commands::plugin::run(command, env),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_internally_consistent() {
        Cli::command().debug_assert();
    }
}
