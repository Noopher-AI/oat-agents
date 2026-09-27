// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

use crate::environment::Environment;
use crate::error::{codes, err};
use crate::event_log::now_iso;
use crate::plugin;
use crate::plugins::{self, PluginPin};
use crate::role::{CoreRole, RoleCatalog};
use crate::trust::{TrustKey, TrustStore};
use anyhow::Result;
use clap::{Args, Subcommand};
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Subcommand, Debug)]
pub enum PluginCommand {
    /// Records that this machine trusts one plugin source and version.
    Trust {
        #[command(subcommand)]
        source: TrustSource,
    },
    /// Lists the plugins a repository is pinned to and whether each is trusted here.
    List(ListArgs),
    /// Validates one plugin directory without trusting it or launching any MCP server.
    Validate(ValidateArgs),
}

#[derive(Subcommand, Debug)]
pub enum TrustSource {
    Embedded(TrustEmbeddedArgs),
    Git(TrustGitArgs),
    Path(TrustPathArgs),
}

#[derive(Args, Debug)]
pub struct TrustEmbeddedArgs {
    #[arg(long)]
    pub name: String,
    /// Defaults to this binary's own version, since that is the only version an embedded
    /// plugin can ever pin to.
    #[arg(long)]
    pub version: Option<String>,
}

#[derive(Args, Debug)]
pub struct TrustGitArgs {
    #[arg(long)]
    pub url: String,
    /// The full commit id, or `latest` to trust every commit the URL serves (ADR-0008).
    #[arg(long)]
    pub commit: String,
}

#[derive(Args, Debug)]
pub struct TrustPathArgs {
    #[arg(long)]
    pub content_hash: String,
}

#[derive(Args, Debug)]
pub struct ListArgs {
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
}

#[derive(Args, Debug)]
pub struct ValidateArgs {
    /// The plugin directory to parse and validate.
    #[arg(long)]
    pub path: PathBuf,
}

pub fn run(command: PluginCommand, env: &dyn Environment) -> Result<Value> {
    match command {
        PluginCommand::Trust { source } => trust(source, env),
        PluginCommand::List(args) => list(args, env),
        PluginCommand::Validate(args) => validate(args),
    }
}

fn validate(args: ValidateArgs) -> Result<Value> {
    let path = args.path.canonicalize().map_err(|e| {
        err(
            crate::error::codes::INVALID_INPUT,
            format!("invalid plugin --path: {e}"),
        )
    })?;
    let (catalog, plugins) = plugin::load_catalog(std::slice::from_ref(&path))
        .map_err(|errors| plugin::errors_to_cli_failure(&errors))?;
    let roles = catalog
        .role_names()
        .into_iter()
        .map(|name| {
            let role = catalog.role(&name).expect("role name came from catalog");
            json!({
                "name": name,
                "mcp_servers": role.mcp_servers.iter().map(|server| server.name.clone()).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let core_roles = [CoreRole::Meta, CoreRole::Console]
        .into_iter()
        .map(|kind| {
            let role = catalog.core_role(kind).expect("valid plugins supply both core roles");
            json!({
                "name": kind.name(),
                "mcp_servers": role.mcp_servers.iter().map(|server| server.name.clone()).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "valid": true,
        "plugins": plugins.iter().map(|plugin| json!({"name": plugin.name})).collect::<Vec<_>>(),
        "mcp_servers": catalog.mcp_server_names(),
        "roles": roles,
        "core_roles": core_roles,
    }))
}

fn trust(source: TrustSource, env: &dyn Environment) -> Result<Value> {
    let key = match source {
        TrustSource::Embedded(args) => TrustKey::Embedded {
            name: args.name,
            version: args.version.unwrap_or_else(|| crate::plugins::embedded::example_version().to_string()),
        },
        TrustSource::Git(args) => TrustKey::Git { url: args.url, commit: args.commit },
        TrustSource::Path(args) => TrustKey::Path { content_hash: args.content_hash },
    };
    let mut store = TrustStore::open(env)?;
    let already_trusted = store.is_trusted(&key);
    store.grant(key.clone(), now_iso())?;
    Ok(json!({
        "trusted": true,
        "already_trusted": already_trusted,
        "source": key.source_label(),
        "version": key.version(),
    }))
}

fn list(args: ListArgs, env: &dyn Environment) -> Result<Value> {
    let repo = args
        .repo
        .canonicalize()
        .map_err(|e| err(codes::INVALID_INPUT, format!("invalid --repo: {e}")))?;
    let Some((config_path, pins)) = plugins::load_pins(&repo)? else {
        return Ok(json!({
            "repo": repo.to_string_lossy(),
            "initialised": false,
            "plugins": [],
        }));
    };

    let home = env
        .home_dir()
        .ok_or_else(|| err(codes::INTERNAL_ERROR, "no home directory available"))?;
    let trust = TrustStore::open(env)?;

    let mut entries = Vec::new();
    for pin in &pins {
        entries.push(describe_pin(&repo, &home, pin, &trust));
    }

    Ok(json!({
        "repo": repo.to_string_lossy(),
        "config": config_path.to_string_lossy(),
        "initialised": true,
        "plugins": entries,
    }))
}

fn describe_pin(repo: &std::path::Path, home: &std::path::Path, pin: &PluginPin, trust: &TrustStore) -> Value {
    match plugins::resolve_pin(repo, home, pin) {
        Ok(resolved) => json!({
            "name": resolved.declared_name,
            "source": resolved.source_label(),
            "version": resolved.version(),
            "trusted": trust.is_trusted(&resolved.trust_key),
            "trust_command": resolved.trust_key.trust_command(),
        }),
        Err(error) => {
            let pin_name = match pin {
                PluginPin::Embedded { name } | PluginPin::Git { name, .. } | PluginPin::Path { name, .. } => name.clone(),
            };
            json!({
                "name": pin_name,
                "error": error.to_string(),
            })
        }
    }
}
