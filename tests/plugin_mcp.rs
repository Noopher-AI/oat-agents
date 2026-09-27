// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

use clap::Parser;
use oat_agents::environment::TestEnvironment;
use oat_agents::plugin::load_catalog;
use oat_agents::plugins::snapshot::{catalog_from_snapshot, snapshot_plugins};
use oat_agents::plugins::ResolvedPlugin;
use oat_agents::role::{CoreRole, RoleCatalog, StartLocation};
use oat_agents::trust::TrustKey;
use oat_agents::{execute, Cli};
use std::fs;
use std::path::{Path, PathBuf};

fn write_plugin(
    root: &Path,
    name: &str,
    role_name: &str,
    manifest_mcp: &str,
    role_mcp: &str,
) -> PathBuf {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("oat-plugin.toml"),
        format!("format_version = 1\nname = \"{name}\"\ndescription = \"test\"\n{manifest_mcp}"),
    )
    .unwrap();
    let role_dir = dir.join("roles").join(role_name);
    fs::create_dir_all(&role_dir).unwrap();
    fs::write(
        role_dir.join("role.toml"),
        format!("description = \"test role\"\nstart = \"fresh\"\n{role_mcp}"),
    )
    .unwrap();
    fs::write(role_dir.join("instructions.md"), "Do the task.\n").unwrap();
    let core_dir = dir.join("core");
    fs::create_dir_all(&core_dir).unwrap();
    fs::write(core_dir.join("oat-meta-instruction.md"), "Coordinate.\n").unwrap();
    fs::write(core_dir.join("oat-console-instruction.md"), "Observe.\n").unwrap();
    dir
}

fn line_server(command: &str) -> String {
    format!(
        "[mcp_servers.line]\ncommand = {command:?}\nargs = [\"--scope\", \"team\"]\n[mcp_servers.line.env]\nMODE = \"local\"\n"
    )
}

#[test]
fn role_bindings_resolve_from_the_plugin_manifest_and_survive_run_snapshots() {
    let temp = tempfile::tempdir().unwrap();
    let source = write_plugin(
        temp.path(),
        "line-team",
        "line-analyst",
        &line_server("oat-line-mcp"),
        "mcp_servers = [\"line\"]\n",
    );
    let (catalog, plugins) = load_catalog(std::slice::from_ref(&source)).unwrap();
    assert_eq!(plugins[0].name, "line-team");
    let role = catalog.role("line-analyst").unwrap();
    assert_eq!(role.start, StartLocation::Fresh);
    assert_eq!(role.mcp_servers.len(), 1);
    assert_eq!(role.mcp_servers[0].name, "line");
    assert_eq!(role.mcp_servers[0].command, "oat-line-mcp");
    assert_eq!(role.mcp_servers[0].args, ["--scope", "team"]);
    assert_eq!(role.mcp_servers[0].env["MODE"], "local");

    let snapshot = temp.path().join("run/plugins");
    snapshot_plugins(
        &[ResolvedPlugin {
            declared_name: "line-team".to_string(),
            dir: source.clone(),
            trust_key: TrustKey::Path {
                content_hash: "sha256:fixture".to_string(),
            },
            version: "fixture-version".to_string(),
        }],
        &snapshot,
    )
    .unwrap();

    fs::write(
        source.join("oat-plugin.toml"),
        format!(
        "format_version = 1\nname = \"line-team\"\ndescription = \"changed after snapshot\"\n{}",
        line_server("changed-command")
    ),
    )
    .unwrap();
    let snapshot_catalog = catalog_from_snapshot(&snapshot).unwrap();
    let snapshot_role = snapshot_catalog.role("line-analyst").unwrap();
    assert_eq!(snapshot_role.mcp_servers[0].command, "oat-line-mcp");
}

#[test]
fn unknown_and_repeated_role_server_bindings_are_rejected_during_plugin_load() {
    let temp = tempfile::tempdir().unwrap();
    let plugin = write_plugin(
        temp.path(),
        "bad-bindings",
        "worker",
        &line_server("line-server"),
        "mcp_servers = [\"missing\", \"line\", \"line\"]\n",
    );
    let errors = load_catalog(&[plugin]).unwrap_err();
    let messages = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(messages.contains("MCP server 'missing'"), "{messages}");
    assert!(
        messages.contains("binds MCP server 'line' more than once"),
        "{messages}"
    );
}

#[test]
fn core_role_bindings_resolve_only_from_the_contributing_plugin() {
    let temp = tempfile::tempdir().unwrap();
    let plugin = write_plugin(
        temp.path(),
        "core-tool",
        "worker",
        &line_server("local-tool"),
        "",
    );
    fs::write(
        plugin.join("core/oat-meta.toml"),
        "mcp_servers = [\"line\"]\n",
    )
    .unwrap();
    let (catalog, _) = load_catalog(&[plugin]).unwrap();
    let meta = catalog.core_role(CoreRole::Meta).unwrap();
    assert_eq!(meta.mcp_servers.len(), 1);
    assert_eq!(meta.mcp_servers[0].name, "line");
    assert!(catalog.role("worker").unwrap().mcp_servers.is_empty());
}

#[test]
fn invalid_server_command_and_environment_names_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let plugin = write_plugin(
        temp.path(),
        "bad-server",
        "worker",
        "[mcp_servers.line]\ncommand = \"  \"\n[mcp_servers.line.env]\n\"BAD-NAME\" = \"value\"\n",
        "mcp_servers = [\"line\"]\n",
    );
    let errors = load_catalog(&[plugin]).unwrap_err();
    let messages = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(messages.contains("command must be non-empty"), "{messages}");
    assert!(
        messages.contains("'BAD-NAME' is not a valid environment variable name"),
        "{messages}"
    );
}

#[test]
fn plugins_cannot_define_the_same_global_mcp_server_name() {
    let temp = tempfile::tempdir().unwrap();
    let first = write_plugin(
        temp.path(),
        "first",
        "worker-a",
        &line_server("one"),
        "mcp_servers = [\"line\"]\n",
    );
    let second = write_plugin(
        temp.path(),
        "second",
        "worker-b",
        &line_server("two"),
        "mcp_servers = [\"line\"]\n",
    );
    let errors = load_catalog(&[first, second]).unwrap_err();
    let messages = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        messages
            .contains("MCP server 'line' is defined by both plugin 'first' and plugin 'second'"),
        "{messages}"
    );
}

#[test]
fn plugin_validate_reports_bindings_without_trusting_or_starting_a_server() {
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("server-was-started");
    let command = format!("touch {}", marker.display());
    let plugin = write_plugin(
        temp.path(),
        "line-team",
        "line-analyst",
        &line_server(&command),
        "mcp_servers = [\"line\"]\n",
    );
    let cli = Cli::try_parse_from([
        "oat-agents",
        "plugin",
        "validate",
        "--path",
        plugin.to_str().unwrap(),
    ])
    .unwrap();
    let env = TestEnvironment::new(temp.path().join("home"));
    let catalog = oat_agents::role::memory::InMemoryCatalogBuilder::new().build();
    let result = execute(cli, &env, &catalog).unwrap();
    assert_eq!(result["valid"], true);
    assert_eq!(result["mcp_servers"][0], "line");
    assert_eq!(result["roles"][0]["name"], "line-analyst");
    assert_eq!(result["roles"][0]["mcp_servers"][0], "line");
    assert!(
        !marker.exists(),
        "validation parses declarations but never executes their command"
    );
}
