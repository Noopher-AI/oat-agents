//! F5's gate end to end: `meta fire`/`console open` against `.oat/plugins.toml`, `plugin
//! trust`/`plugin list`, and what `run.json`/the workflow log record about it (issue's own
//! acceptance criteria).

mod common;

use clap::Parser;
use common::{PluginBuilder, TempRepo, TestWorld};
use oat_agents::error::{codes, to_failure};
use oat_agents::role::memory::InMemoryCatalogBuilder;
use oat_agents::{execute, Cli};

fn parse(args: &[&str]) -> Cli {
    let mut full = vec!["oat-agents"];
    full.extend_from_slice(args);
    Cli::try_parse_from(full).unwrap()
}

fn run(cli: Cli, env: &dyn oat_agents::environment::Environment) -> anyhow::Result<serde_json::Value> {
    let catalog = InMemoryCatalogBuilder::new().build();
    execute(cli, env, &catalog)
}

fn fire(repo: &std::path::Path, world: &TestWorld, name: &str) -> anyhow::Result<serde_json::Value> {
    let cli = parse(&[
        "meta", "fire", "--prompt", "plan", "--repo", &repo.to_string_lossy(), "--name", name, "--agent", "claude",
    ]);
    run(cli, &world.env())
}

#[test]
fn meta_fire_in_an_uninitialised_repository_fails_and_names_init() {
    let repo = TempRepo::new();
    let world = TestWorld::new();

    let error = fire(&repo.path(), &world, "run-a").unwrap_err();
    let failure = to_failure(&error);
    assert_eq!(failure.code, codes::PLUGIN_UNINITIALISED);
    assert!(failure.message.to_lowercase().contains("init"), "{}", failure.message);
}

#[test]
fn an_untrusted_plugin_fails_and_prints_the_trust_command_and_trusting_it_starts_the_run() {
    let repo = TempRepo::new();
    let world = TestWorld::new();

    PluginBuilder::new(&repo.path(), "fixture-plugin", "fixture-plugin")
        .core("oat-meta", "Coordinate.")
        .core("oat-console", "Observe.")
        .role("worker", "fresh", "Do the work.");
    // Pin it (writes .oat/plugins.toml) but do NOT grant trust.
    let path_pin = oat_agents::plugins::path_pin_from_current_content(&repo.path(), "fixture-plugin", "fixture-plugin").unwrap();
    let content_hash = match &path_pin {
        oat_agents::plugins::PluginPin::Path { content_hash, .. } => content_hash.clone(),
        _ => unreachable!(),
    };
    common::write_plugins_config(&repo.path(), &[path_pin]);

    let error = fire(&repo.path(), &world, "run-a").unwrap_err();
    let failure = to_failure(&error);
    assert_eq!(failure.code, codes::PLUGIN_UNTRUSTED);
    assert!(failure.message.contains("fixture-plugin"), "{}", failure.message);
    assert!(
        failure.message.contains("oat-agents plugin trust path --content-hash"),
        "names the exact trust command: {}",
        failure.message
    );
    assert!(failure.message.contains(&content_hash), "{}", failure.message);

    // `plugin trust` records it, and the same `meta fire` now starts.
    let trust_cli = parse(&["plugin", "trust", "path", "--content-hash", &content_hash]);
    run(trust_cli, &world.env()).unwrap();

    let result = fire(&repo.path(), &world, "run-b").unwrap();
    assert_eq!(result["run_id"], "run-b");
}

#[test]
fn changing_the_pins_content_hash_voids_trust() {
    let repo = TempRepo::new();
    let world = TestWorld::new();

    PluginBuilder::new(&repo.path(), "fixture-plugin", "fixture-plugin")
        .core("oat-meta", "Coordinate.")
        .core("oat-console", "Observe.")
        .role("worker", "fresh", "Do the work.")
        .finish(&world, &repo.path(), "fixture-plugin", "fixture-plugin");

    fire(&repo.path(), &world, "run-a").unwrap();

    // Editing the plugin after it was pinned means the pin in `.oat/plugins.toml` no longer
    // matches the plugin's current content; resolving it fails outright.
    std::fs::write(
        repo.path().join("fixture-plugin/roles/worker/instructions.md"),
        "Do the work differently now.",
    )
    .unwrap();
    let error = fire(&repo.path(), &world, "run-b").unwrap_err();
    assert_eq!(to_failure(&error).code, codes::PLUGIN_PIN_MISMATCH);

    // Re-pinning to the new content hash still needs a fresh trust grant: the old grant is for
    // the old hash and does not carry over.
    let new_pin = oat_agents::plugins::path_pin_from_current_content(&repo.path(), "fixture-plugin", "fixture-plugin").unwrap();
    common::write_plugins_config(&repo.path(), &[new_pin]);
    let error = fire(&repo.path(), &world, "run-c").unwrap_err();
    assert_eq!(to_failure(&error).code, codes::PLUGIN_UNTRUSTED, "the changed pin is untrusted again");
}

#[test]
fn two_plugins_defining_one_role_name_fail_the_gate() {
    let repo = TempRepo::new();
    let world = TestWorld::new();

    let pin_a = PluginBuilder::new(&repo.path(), "plugin-a", "plugin-a")
        .core("oat-meta", "Coordinate.")
        .core("oat-console", "Observe.")
        .role("worker", "fresh", "Do the work.")
        .finish(&world, &repo.path(), "plugin-a", "plugin-a");
    let pin_b = PluginBuilder::new(&repo.path(), "plugin-b", "plugin-b")
        .role("worker", "fresh", "Do the work differently.")
        .finish(&world, &repo.path(), "plugin-b", "plugin-b");

    common::write_plugins_config(&repo.path(), &[pin_a, pin_b]);

    let error = fire(&repo.path(), &world, "run-a").unwrap_err();
    let failure = to_failure(&error);
    assert_eq!(failure.code, codes::PLUGIN_LOAD_FAILED);
    assert!(failure.message.contains("worker"));
    assert!(failure.message.contains("plugin-a"));
    assert!(failure.message.contains("plugin-b"));
}

#[test]
fn a_started_runs_run_json_and_workflow_log_name_every_plugin_with_its_exact_version() {
    let repo = TempRepo::new();
    let world = TestWorld::new();

    let pin = PluginBuilder::new(&repo.path(), "fixture-plugin", "fixture-plugin")
        .core("oat-meta", "Coordinate.")
        .core("oat-console", "Observe.")
        .role("worker", "fresh", "Do the work.")
        .finish(&world, &repo.path(), "fixture-plugin", "fixture-plugin");
    let expected_hash = match &pin {
        oat_agents::plugins::PluginPin::Path { content_hash, .. } => content_hash.clone(),
        _ => unreachable!(),
    };

    let result = fire(&repo.path(), &world, "run-a").unwrap();
    let run_id = result["run_id"].as_str().unwrap().to_string();

    let store = oat_agents::store::Store::open(&world.env()).unwrap();
    let run_record = store.load_run(&run_id).unwrap();
    assert_eq!(run_record.plugins.len(), 1);
    assert_eq!(run_record.plugins[0].name, "fixture-plugin");
    assert_eq!(run_record.plugins[0].version, expected_hash);

    let log = oat_agents::event_log::EventLog::open(&world.env());
    let entries = log.read_run(&run_id).unwrap();
    let plugin_entry = entries
        .iter()
        .find(|e| e.event == "plugins_resolved")
        .expect("a plugins_resolved log entry");
    let logged_plugins = plugin_entry.details.as_ref().unwrap()["plugins"].as_array().unwrap();
    assert_eq!(logged_plugins.len(), 1);
    assert_eq!(logged_plugins[0]["version"], expected_hash);
}

#[test]
fn plugin_list_reports_trust_status_without_starting_anything() {
    let repo = TempRepo::new();
    let world = TestWorld::new();

    PluginBuilder::new(&repo.path(), "fixture-plugin", "fixture-plugin")
        .core("oat-meta", "Coordinate.")
        .core("oat-console", "Observe.")
        .role("worker", "fresh", "Do the work.");
    let real_pin = oat_agents::plugins::path_pin_from_current_content(&repo.path(), "fixture-plugin", "fixture-plugin").unwrap();
    common::write_plugins_config(&repo.path(), &[real_pin]);

    let list_cli = parse(&["plugin", "list", "--repo", &repo.path().to_string_lossy()]);
    let result = run(list_cli, &world.env()).unwrap();
    let plugins = result["plugins"].as_array().unwrap();
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0]["name"], "fixture-plugin");
    assert_eq!(plugins[0]["trusted"], false);
}

#[test]
fn the_embedded_example_plugin_resolves_trusts_and_starts_a_run() {
    let repo = TempRepo::new();
    let world = TestWorld::new();

    run(parse(&["init", "--repo", &repo.path().to_string_lossy(), "--embedded"]), &world.env()).unwrap();

    let version = oat_agents::plugins::embedded::example_version().to_string();
    let error = fire(&repo.path(), &world, "run-a").unwrap_err();
    let failure = to_failure(&error);
    assert_eq!(failure.code, codes::PLUGIN_UNTRUSTED);
    assert!(failure.message.contains(&format!(
        "oat-agents plugin trust embedded --name example --version {version}"
    )));

    run(
        parse(&["plugin", "trust", "embedded", "--name", "example", "--version", &version]),
        &world.env(),
    )
    .unwrap();

    let result = fire(&repo.path(), &world, "run-b").unwrap();
    let run_id = result["run_id"].as_str().unwrap().to_string();
    let store = oat_agents::store::Store::open(&world.env()).unwrap();
    let run_record = store.load_run(&run_id).unwrap();
    assert_eq!(run_record.plugins, vec![oat_agents::store::PluginRecord {
        name: "example".to_string(),
        source: "embedded".to_string(),
        version,
    }]);
}

#[test]
fn the_console_launch_is_gated_exactly_like_meta_fire() {
    let world = TestWorld::new();
    let env = world.env();
    let repo = tempfile::tempdir().unwrap();

    let error = oat_agents::console::open(&env, repo.path(), oat_agents::role::Backend::Claude).unwrap_err();
    assert_eq!(to_failure(&error).code, codes::PLUGIN_UNINITIALISED);

    common::install_console_plugin(&world, repo.path(), "team instructions");
    // Trust granted by `install_console_plugin`'s `finish`; the console now opens.
    let record = oat_agents::console::open(&env, repo.path(), oat_agents::role::Backend::Claude).unwrap();
    assert_eq!(record.backend, "claude");
}
