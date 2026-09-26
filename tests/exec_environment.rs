mod common;

use common::{TempRepo, TestWorld};
use clap::Parser;
use oat_agents::env::integration::ExecEnvironments;
use oat_agents::env::EnvRecord;
use oat_agents::environment::Environment;
use oat_agents::event_log::EventLog;
use oat_agents::role::memory::InMemoryCatalogBuilder;
use oat_agents::role::{CoreRole, CoreRoleDefinition, RoleDefinition, StartLocation};
use oat_agents::{execute_with_exec, Cli};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// A stand-in for a real cluster: hands back a fixed environment record and a fixed ledger
/// report, so `role fire`'s wiring can be tested without Kubernetes.
struct FakeExec {
    record: EnvRecord,
    reusable: serde_json::Value,
    calls: Mutex<Vec<String>>,
}

impl FakeExec {
    fn new() -> Self {
        Self {
            record: EnvRecord {
                env_id: "oat-fake-worker-1".to_string(),
                run_id: Some("run".to_string()),
                role: "worker".to_string(),
                profile: "local".to_string(),
                worktree: PathBuf::from("/w"),
                container_path: PathBuf::from("/w"),
                image_ref: "oat-w:abc".to_string(),
                image_id: "sha256:fakeimage".to_string(),
                pod: "oat-fake-worker-1".to_string(),
                namespace: "agents".to_string(),
                context: "local".to_string(),
                created_ms: 1,
            },
            reusable: serde_json::json!({"entries": [], "note": "fake"}),
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl ExecEnvironments for FakeExec {
    fn ensure(
        &self,
        _environment: &dyn Environment,
        _log: &EventLog,
        _run_id: &str,
        _profile: &str,
        role: &str,
        _worktree: &Path,
    ) -> anyhow::Result<EnvRecord> {
        self.calls.lock().unwrap().push(format!("ensure:{role}"));
        Ok(self.record.clone())
    }

    fn reusable(&self, _environment: &dyn Environment, _worktree: &Path, _against: &EnvRecord) -> serde_json::Value {
        self.reusable.clone()
    }
}

fn write_exec_profile(repo: &Path, home: &Path) {
    std::fs::create_dir_all(repo.join(".oat")).unwrap();
    std::fs::write(repo.join(".oat/exec.toml"), "default_profile = \"local\"\n").unwrap();
    std::fs::create_dir_all(home.join(".oat")).unwrap();
    std::fs::write(
        home.join(".oat/exec.toml"),
        "[profile.local]\ncontext = \"test-context\"\nnamespace = \"test-namespace\"\n",
    )
    .unwrap();
}

fn write_exec_profile_with_kubeconfig(repo: &Path, home: &Path, kubeconfig: &Path) {
    std::fs::create_dir_all(repo.join(".oat")).unwrap();
    std::fs::write(repo.join(".oat/exec.toml"), "default_profile = \"local\"\n").unwrap();
    std::fs::create_dir_all(home.join(".oat")).unwrap();
    std::fs::write(
        home.join(".oat/exec.toml"),
        format!(
            "[profile.local]\ncontext = \"test-context\"\nnamespace = \"test-namespace\"\nkubeconfig = {:?}\n",
            kubeconfig
        ),
    )
    .unwrap();
}

fn catalog_with(exec_environment: bool, prior_verification: bool) -> oat_agents::role::memory::InMemoryCatalog {
    InMemoryCatalogBuilder::new()
        .with_role(RoleDefinition {
            name: "worker".to_string(),
            instructions: "Write the pottery this task asks for.".to_string(),
            models: BTreeMap::new(),
            skills: Vec::new(),
            start: StartLocation::Fresh,
            exec_environment,
            prior_verification,
        })
        .with_core_role(
            CoreRole::Meta,
            CoreRoleDefinition {
                instructions: "Coordinate the pottery workshop.".to_string(),
                models: BTreeMap::new(),
                skills: Vec::new(),
            },
        )
        .with_core_role(
            CoreRole::Console,
            CoreRoleDefinition {
                instructions: "Observe the pottery workshop.".to_string(),
                models: BTreeMap::new(),
                skills: Vec::new(),
            },
        )
        .build()
}

fn parse(args: &[&str]) -> Cli {
    let mut full = vec!["oat-agents"];
    full.extend_from_slice(args);
    Cli::try_parse_from(full).unwrap()
}

/// The Run's plugin (F5's gate) must itself carry the `exec_environment`/`prior_verification`
/// flags each test exercises: `role fire` now rebuilds its catalog from the Run's plugin
/// snapshot, not from the `catalog_with(...)` fixture passed alongside it.
fn fire_run(world: &TestWorld, repo: &Path, exec_environment: bool, prior_verification: bool) -> String {
    common::PluginBuilder::new(repo, "fixture-plugin", "fixture-plugin")
        .role_with(
            "worker",
            "fresh",
            "Write the pottery this task asks for.",
            &[],
            exec_environment,
            prior_verification,
        )
        .with_default_core()
        .finish(world, repo, "fixture-plugin", "fixture-plugin");
    let env = world.env();
    let catalog = catalog_with(exec_environment, prior_verification);
    let fake = FakeExec::new();
    let cli = parse(&[
        "meta", "fire", "--prompt", "plan", "--repo", &repo.to_string_lossy(), "--name", "run-a", "--agent", "claude",
    ]);
    execute_with_exec(cli, &env, &catalog, &fake).unwrap();
    "run-a".to_string()
}

#[test]
fn exec_environment_false_gets_no_environment_even_when_the_run_has_a_profile() {
    let repo = TempRepo::new();
    let world = TestWorld::new();
    write_exec_profile(&repo.path(), &world.home);
    let run_id = fire_run(&world, &repo.path(), false, false);

    let catalog = catalog_with(false, false);
    let fake = FakeExec::new();
    let env_with_run = world.env().with_var("OAT_RUN_ID", &run_id);
    let role_fire = parse(&["role", "fire", "worker", "--name", "w1", "--prompt", "do work"]);
    let result = execute_with_exec(role_fire, &env_with_run, &catalog, &fake).unwrap();

    assert!(fake.calls.lock().unwrap().is_empty(), "an opted-out role never reaches the provider");
    let dispatch_dir = PathBuf::from(result["dispatch_dir"].as_str().unwrap());
    let prompt = std::fs::read_to_string(dispatch_dir.join("prompt.md")).unwrap();
    assert!(!prompt.contains("<execution-environment>"), "no environment was bound: {prompt}");
}

#[test]
fn exec_environment_true_gets_one_when_the_run_has_a_profile() {
    let repo = TempRepo::new();
    let world = TestWorld::new();
    write_exec_profile(&repo.path(), &world.home);
    let run_id = fire_run(&world, &repo.path(), true, false);

    let catalog = catalog_with(true, false);
    let fake = FakeExec::new();
    let env_with_run = world.env().with_var("OAT_RUN_ID", &run_id);
    let role_fire = parse(&["role", "fire", "worker", "--name", "w1", "--prompt", "do work"]);
    let result = execute_with_exec(role_fire, &env_with_run, &catalog, &fake).unwrap();

    assert_eq!(fake.calls.lock().unwrap().as_slice(), ["ensure:worker"]);
    let dispatch_dir = PathBuf::from(result["dispatch_dir"].as_str().unwrap());
    let prompt = std::fs::read_to_string(dispatch_dir.join("prompt.md")).unwrap();
    assert!(prompt.contains("<execution-environment>"), "{prompt}");
    assert!(prompt.contains("sha256:fakeimage"), "{prompt}");
    assert!(!prompt.contains("Prior verification"), "prior_verification is false: {prompt}");
}

#[test]
fn prior_verification_true_carries_the_ledgers_reusable_entries_and_false_does_not() {
    let repo = TempRepo::new();
    let world = TestWorld::new();
    write_exec_profile(&repo.path(), &world.home);
    let run_id = fire_run(&world, &repo.path(), true, true);

    let catalog = catalog_with(true, true);
    let fake = FakeExec::new();
    let env_with_run = world.env().with_var("OAT_RUN_ID", &run_id);
    let role_fire = parse(&["role", "fire", "worker", "--name", "w1", "--prompt", "do work"]);
    let result = execute_with_exec(role_fire, &env_with_run, &catalog, &fake).unwrap();
    let dispatch_dir = PathBuf::from(result["dispatch_dir"].as_str().unwrap());
    let prompt = std::fs::read_to_string(dispatch_dir.join("prompt.md")).unwrap();
    assert!(prompt.contains("Prior verification"), "{prompt}");
    assert!(prompt.contains("\"note\": \"fake\""), "the ledger's reusable entries are in the prompt: {prompt}");
}

#[test]
fn exec_environment_false_never_gets_prior_verification_either() {
    let repo = TempRepo::new();
    let world = TestWorld::new();
    write_exec_profile(&repo.path(), &world.home);
    let run_id = fire_run(&world, &repo.path(), false, true);

    let catalog = catalog_with(false, true);
    let fake = FakeExec::new();
    let env_with_run = world.env().with_var("OAT_RUN_ID", &run_id);
    let role_fire = parse(&["role", "fire", "worker", "--name", "w1", "--prompt", "do work"]);
    let result = execute_with_exec(role_fire, &env_with_run, &catalog, &fake).unwrap();
    let dispatch_dir = PathBuf::from(result["dispatch_dir"].as_str().unwrap());
    let prompt = std::fs::read_to_string(dispatch_dir.join("prompt.md")).unwrap();
    assert!(!prompt.contains("Prior verification"), "{prompt}");
    assert!(fake.calls.lock().unwrap().is_empty());
}

#[test]
fn env_doctor_reports_what_is_missing_on_a_machine_with_no_cluster() {
    // No real kubeconfig can be reached from a test process: point the profile's own
    // `kubeconfig` field at a path that does not exist, so the report is deterministic wherever
    // this runs. Mutating the process-wide `KUBECONFIG` variable would race other tests running
    // in parallel threads in this same binary.
    let directory = tempfile::tempdir().unwrap();
    let missing_kubeconfig = directory.path().join("no-such-kubeconfig");

    let repo = directory.path().join("repo");
    let home = directory.path().join("home");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    write_exec_profile_with_kubeconfig(&repo, &home, &missing_kubeconfig);
    let environment = oat_agents::environment::TestEnvironment::new(home);
    let log = EventLog::for_dir(None);

    let result = oat_agents::env::commands::execute(
        &environment,
        &log,
        oat_agents::env::commands::EnvSubcommand::Doctor(oat_agents::env::commands::EnvDoctorArgs {
            profile: None,
            repo,
        }),
    );

    let report = result.expect("doctor reports rather than failing when the cluster is unreachable");
    assert_eq!(report["kubeconfig_valid"], false);
    assert!(report["cluster_error"].as_str().is_some_and(|s| !s.is_empty()), "{report}");
    assert_eq!(report["profile"], "local");
    assert!(report.get("tools").is_some(), "{report}");
}
