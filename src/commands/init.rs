// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

//! `oat-agents init [--local]` (ticket Scope): declares a repository's pinned plugins, and
//! optionally its execution profile, without ever overwriting an existing configuration
//! without saying so.

use crate::env::config::WorkspaceKind;
use crate::env::scaffold::{self, MachineFacts};
use crate::environment::Environment;
use crate::error::{codes, err};
use crate::plugins::{self, PluginPin};
use anyhow::Result;
use clap::Args;
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Args, Debug)]
pub struct InitArgs {
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    /// Keeps `.oat/plugins.toml` under `.git/` instead of inside the repository, so `init
    /// --local` leaves `git status` clean.
    #[arg(long)]
    pub local: bool,
    /// Overwrites an existing configuration instead of refusing to touch it.
    #[arg(long)]
    pub force: bool,
    /// Pins the embedded example plugin. The default when no other `--git` or `--path` is
    /// given.
    #[arg(long)]
    pub embedded: bool,
    /// A git-pinned plugin: its URL, the full commit id to pin (or `latest`, to follow its
    /// default branch, ADR-0008), and the name it must declare. Repeatable.
    #[arg(long, num_args = 3, value_names = ["URL", "COMMIT", "NAME"])]
    pub git: Vec<String>,
    /// An in-repository plugin: its path relative to the repository root, and the name it must
    /// declare. Its content hash is computed now, from whatever is on disk. Repeatable.
    #[arg(long, num_args = 2, value_names = ["PATH", "NAME"])]
    pub path: Vec<String>,
    /// Also scaffolds this execution profile (F2) with `--exec-context`/`--exec-namespace`.
    #[arg(long, value_name = "NAME")]
    pub exec_profile: Option<String>,
    #[arg(long)]
    pub exec_context: Option<String>,
    #[arg(long)]
    pub exec_namespace: Option<String>,
    #[arg(long, value_enum, default_value_t = WorkspaceKind::Hostpath)]
    pub exec_workspace: WorkspaceKind,
}

pub fn run(args: InitArgs, env: &dyn Environment) -> Result<Value> {
    let repo = args
        .repo
        .canonicalize()
        .map_err(|e| err(codes::INVALID_INPUT, format!("invalid --repo: {e}")))?;

    let config_path = if args.local {
        plugins::local_config_path(&repo).ok_or_else(|| {
            err(
                codes::INVALID_INPUT,
                format!("'{}' is not a git repository; `init --local` needs one", repo.display()),
            )
        })?
    } else {
        plugins::repo_config_path(&repo)
    };

    if config_path.is_file() && !args.force {
        return Err(err(
            codes::PLUGIN_ALREADY_INITIALISED,
            format!(
                "'{}' already declares plugins; pass --force to overwrite it",
                config_path.display()
            ),
        ));
    }

    let pins = build_pins(&args, &repo)?;

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| err(codes::INTERNAL_ERROR, format!("could not create {}: {e}", parent.display())))?;
    }
    std::fs::write(&config_path, plugins::render_config(&pins))
        .map_err(|e| err(codes::INTERNAL_ERROR, format!("could not write {}: {e}", config_path.display())))?;

    let exec_report = match &args.exec_profile {
        Some(profile_name) => Some(scaffold_exec_profile(env, &repo, profile_name, &args)?),
        None => None,
    };

    Ok(json!({
        "repo": repo.to_string_lossy(),
        "config": config_path.to_string_lossy(),
        "local": args.local,
        "plugins": pins.iter().map(describe_pin).collect::<Vec<_>>(),
        "exec_profile": exec_report,
    }))
}

fn build_pins(args: &InitArgs, repo: &std::path::Path) -> Result<Vec<PluginPin>> {
    let mut pins = Vec::new();

    if args.embedded {
        pins.push(plugins::embedded_pin("example")?);
    }
    for chunk in args.git.chunks_exact(3) {
        let [url, commit, name] = [&chunk[0], &chunk[1], &chunk[2]];
        pins.push(plugins::git_pin(name.clone(), url.clone(), commit.clone())?);
    }
    for chunk in args.path.chunks_exact(2) {
        let [path, name] = [&chunk[0], &chunk[1]];
        pins.push(plugins::path_pin_from_current_content(repo, path.clone(), name.clone())?);
    }

    if pins.is_empty() {
        pins.push(plugins::embedded_pin("example")?);
    }
    Ok(pins)
}

fn scaffold_exec_profile(env: &dyn Environment, repo: &std::path::Path, profile_name: &str, args: &InitArgs) -> Result<Value> {
    let context = args
        .exec_context
        .clone()
        .ok_or_else(|| err(codes::INVALID_CLI_ARGUMENTS, "--exec-profile requires --exec-context"))?;
    let namespace = args
        .exec_namespace
        .clone()
        .ok_or_else(|| err(codes::INVALID_CLI_ARGUMENTS, "--exec-profile requires --exec-namespace"))?;
    let facts = MachineFacts { context, namespace, workspace: args.exec_workspace };
    let report = scaffold::scaffold(env, repo, profile_name, &facts)?;
    let machine_path = env
        .home_dir()
        .map(|home| home.join(crate::env::config::MACHINE_CONFIG).to_string_lossy().to_string());
    Ok(json!({
        "profile": profile_name,
        "repo_file": format!("{:?}", report.repo_file),
        "repo_path": repo.join(crate::env::config::REPO_CONFIG).to_string_lossy(),
        "machine_file": format!("{:?}", report.machine_file),
        "machine_path": machine_path,
        "next": format!(
            "finish the profile's image settings in the machine file (every optional field is there, \
             commented out), then check it with `oat-agents env doctor --profile {profile_name}` and \
             `oat-agents env image --profile {profile_name}`"
        ),
    }))
}

fn describe_pin(pin: &PluginPin) -> Value {
    match pin {
        PluginPin::Embedded { name } => json!({"source": "embedded", "name": name}),
        PluginPin::Git { name, url, commit } => json!({"source": "git", "name": name, "url": url, "commit": commit}),
        PluginPin::Path { name, path, content_hash } => {
            json!({"source": "path", "name": name, "path": path, "content_hash": content_hash})
        }
    }
}
