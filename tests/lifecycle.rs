mod common;

use common::{TempRepo, TestWorld};
use oat_agents::error::{codes, to_failure};
use oat_agents::role::memory::InMemoryCatalogBuilder;
use oat_agents::role::{CoreRole, CoreRoleDefinition, RoleDefinition, StartLocation};
use oat_agents::{execute, Cli};
use clap::Parser;
use std::collections::BTreeMap;

fn fixture_catalog() -> oat_agents::role::memory::InMemoryCatalog {
    InMemoryCatalogBuilder::new()
        .with_role(RoleDefinition {
            name: "worker".to_string(),
            instructions: "Write the pottery this task asks for.".to_string(),
            models: BTreeMap::new(),
            skills: Vec::new(),
            start: StartLocation::Fresh,
            exec_environment: false,
            prior_verification: false,
        })
        .with_role(RoleDefinition {
            name: "reviewer".to_string(),
            instructions: "Review the pottery someone else made.".to_string(),
            models: BTreeMap::new(),
            skills: Vec::new(),
            start: StartLocation::Existing,
            exec_environment: false,
            prior_verification: false,
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

fn run_json(cli: Cli, env: &dyn oat_agents::environment::Environment, catalog: &dyn oat_agents::role::RoleCatalog) -> serde_json::Value {
    execute(cli, env, catalog).unwrap()
}

#[test]
fn meta_fire_creates_a_run_a_worktree_and_a_session() {
    let repo = TempRepo::new();
    let world = TestWorld::new();
    let env = world.env();
    let catalog = fixture_catalog();

    let cli = parse(&[
        "meta",
        "fire",
        "--prompt",
        "Ship the glaze feature",
        "--repo",
        &repo.path().to_string_lossy(),
        "--name",
        "glaze",
        "--agent",
        "claude",
    ]);

    let result = run_json(cli, &env, &catalog);
    let run_id = result["run_id"].as_str().unwrap().to_string();
    assert_eq!(run_id, "glaze");

    let worktree = std::path::PathBuf::from(result["worktree"].as_str().unwrap());
    assert!(worktree.exists(), "the coordinator's worktree was created");
    assert_eq!(result["branch"], "oat/glaze/meta");

    let calls = world.tmux_calls();
    let new_session_call = calls
        .iter()
        .find(|c| c.contains("new-session"))
        .expect("a new-session call was recorded");
    assert!(
        new_session_call.contains("oat_meta-"),
        "session named after the meta hash_id: {new_session_call}"
    );
}

#[test]
fn fresh_role_gets_its_own_child_worktree() {
    let repo = TempRepo::new();
    let world = TestWorld::new();
    let env = world.env();
    let catalog = fixture_catalog();

    let fire = parse(&[
        "meta", "fire", "--prompt", "plan", "--repo", &repo.path().to_string_lossy(),
        "--name", "run-a", "--agent", "claude",
    ]);
    run_json(fire, &env, &catalog);

    let env_with_run = world.env().with_var("OAT_RUN_ID", "run-a");
    let role_fire = parse(&["role", "fire", "worker", "--name", "w1", "--prompt", "do work"]);
    let result = run_json(role_fire, &env_with_run, &catalog);

    let worktree = std::path::PathBuf::from(result["worktree"].as_str().unwrap());
    assert!(worktree.exists());
    assert!(worktree.to_string_lossy().contains(".oat-run-a-w1"));
    assert_eq!(result["branch"], "oat/run-a/w1");
}

#[test]
fn role_fire_defaults_to_the_run_s_recorded_backend() {
    let repo = TempRepo::new();
    let world = TestWorld::new();
    let env = world.env();
    let catalog = InMemoryCatalogBuilder::new()
        .with_role(RoleDefinition {
            name: "worker".to_string(),
            instructions: "Write the pottery this task asks for.".to_string(),
            models: BTreeMap::new(),
            skills: vec![oat_agents::role::SkillRef {
                name: "glazing".to_string(),
                files: vec![oat_agents::role::SkillFile {
                    relative_path: std::path::PathBuf::from("SKILL.md"),
                    contents: b"# Glazing".to_vec(),
                    executable: false,
                }],
            }],
            start: StartLocation::Fresh,
            exec_environment: false,
            prior_verification: false,
        })
        .with_core_role(
            CoreRole::Meta,
            CoreRoleDefinition {
                instructions: "Coordinate the pottery workshop.".to_string(),
                models: BTreeMap::new(),
                skills: Vec::new(),
            },
        )
        .build();

    let fire = parse(&[
        "meta", "fire", "--prompt", "plan", "--repo", &repo.path().to_string_lossy(),
        "--name", "run-codex", "--agent", "codex",
    ]);
    run_json(fire, &env, &catalog);

    let env_with_run = world.env().with_var("OAT_RUN_ID", "run-codex");
    let role_fire = parse(&["role", "fire", "worker", "--name", "w1", "--prompt", "do work"]);
    let result = run_json(role_fire, &env_with_run, &catalog);

    let worktree = std::path::PathBuf::from(result["worktree"].as_str().unwrap());
    let dispatch_dir = std::path::PathBuf::from(result["dispatch_dir"].as_str().unwrap());

    let script = std::fs::read_to_string(dispatch_dir.join("launch.sh")).unwrap();
    assert!(script.contains("codex"), "launch script targets Codex: {script}");
    assert!(!script.contains("claude"), "launch script does not target Claude: {script}");

    let prompt = std::fs::read_to_string(dispatch_dir.join("prompt.md")).unwrap();
    assert!(prompt.contains("Codex"), "prompt names Codex as the backend: {prompt}");

    assert!(
        worktree.join(".agents/skills/glazing/SKILL.md").exists(),
        "the role's skill is materialized under .agents/skills/, Codex's convention"
    );
    assert!(
        !worktree.join(".claude/skills").exists(),
        "no Claude-only skill directory is created for a Codex-backed role"
    );
}

#[test]
fn existing_role_runs_in_the_named_worktree() {
    let repo = TempRepo::new();
    let world = TestWorld::new();
    let env = world.env();
    let catalog = fixture_catalog();

    let fire = parse(&[
        "meta", "fire", "--prompt", "plan", "--repo", &repo.path().to_string_lossy(),
        "--name", "run-b", "--agent", "claude",
    ]);
    let meta_result = run_json(fire, &env, &catalog);
    let meta_worktree = meta_result["worktree"].as_str().unwrap().to_string();

    let env_with_run = world.env().with_var("OAT_RUN_ID", "run-b");
    let role_fire = parse(&[
        "role", "fire", "reviewer", "--from", &meta_worktree, "--prompt", "review this",
    ]);
    let result = run_json(role_fire, &env_with_run, &catalog);

    assert_eq!(result["worktree"].as_str().unwrap(), meta_worktree);
}

#[test]
fn from_is_refused_for_a_fresh_role() {
    let repo = TempRepo::new();
    let world = TestWorld::new();
    let env = world.env();
    let catalog = fixture_catalog();

    let fire = parse(&[
        "meta", "fire", "--prompt", "plan", "--repo", &repo.path().to_string_lossy(),
        "--name", "run-c", "--agent", "claude",
    ]);
    let meta_result = run_json(fire, &env, &catalog);
    let meta_worktree = meta_result["worktree"].as_str().unwrap().to_string();

    let env_with_run = world.env().with_var("OAT_RUN_ID", "run-c");
    let role_fire = parse(&[
        "role", "fire", "worker", "--from", &meta_worktree, "--prompt", "do work",
    ]);
    let err = execute(role_fire, &env_with_run, &catalog).unwrap_err();
    assert_eq!(to_failure(&err).code, codes::INVALID_ROLE_OPTION);
}

#[test]
fn from_is_required_for_an_existing_role() {
    let repo = TempRepo::new();
    let world = TestWorld::new();
    let env = world.env();
    let catalog = fixture_catalog();

    let fire = parse(&[
        "meta", "fire", "--prompt", "plan", "--repo", &repo.path().to_string_lossy(),
        "--name", "run-d", "--agent", "claude",
    ]);
    run_json(fire, &env, &catalog);

    let env_with_run = world.env().with_var("OAT_RUN_ID", "run-d");
    let role_fire = parse(&["role", "fire", "reviewer", "--prompt", "review this"]);
    let err = execute(role_fire, &env_with_run, &catalog).unwrap_err();
    assert_eq!(to_failure(&err).code, codes::ROLE_SOURCE_REQUIRED);
}

#[test]
fn unknown_role_names_the_role_and_the_known_ones() {
    let repo = TempRepo::new();
    let world = TestWorld::new();
    let env = world.env();
    let catalog = fixture_catalog();

    let fire = parse(&[
        "meta", "fire", "--prompt", "plan", "--repo", &repo.path().to_string_lossy(),
        "--name", "run-e", "--agent", "claude",
    ]);
    run_json(fire, &env, &catalog);

    let env_with_run = world.env().with_var("OAT_RUN_ID", "run-e");
    let role_fire = parse(&["role", "fire", "sculptor", "--prompt", "sculpt"]);
    let err = execute(role_fire, &env_with_run, &catalog).unwrap_err();
    let failure = to_failure(&err);
    assert_eq!(failure.code, codes::UNKNOWN_ROLE);
    assert!(failure.message.contains("sculptor"));
    assert!(failure.message.contains("worker"));
    assert!(failure.message.contains("reviewer"));
}

#[test]
fn a_silent_role_still_gets_the_preamble_and_the_protocol_before_its_instructions() {
    let repo = TempRepo::new();
    let world = TestWorld::new();
    let env = world.env();
    let catalog = fixture_catalog();

    let fire = parse(&[
        "meta", "fire", "--prompt", "plan", "--repo", &repo.path().to_string_lossy(),
        "--name", "run-f", "--agent", "claude",
    ]);
    run_json(fire, &env, &catalog);

    let env_with_run = world.env().with_var("OAT_RUN_ID", "run-f");
    let role_fire = parse(&["role", "fire", "worker", "--name", "w1", "--prompt", "THE-TASK-TEXT"]);
    let result = run_json(role_fire, &env_with_run, &catalog);

    let dispatch_dir = std::path::PathBuf::from(result["dispatch_dir"].as_str().unwrap());
    let prompt = std::fs::read_to_string(dispatch_dir.join("prompt.md")).unwrap();

    let preamble_at = prompt.find("Launch").expect("preamble present");
    let protocol_at = prompt.find("Role protocol").expect("role protocol present");
    let instructions_at = prompt.find("Write the pottery this task asks for.").expect("instructions present");
    let task_at = prompt.find("THE-TASK-TEXT").expect("task present");

    assert!(preamble_at < protocol_at, "preamble before protocol");
    assert!(protocol_at < instructions_at, "protocol before instructions");
    assert!(instructions_at < task_at, "instructions before task");
}

#[test]
fn a_run_goes_from_delegation_to_finish() {
    let repo = TempRepo::new();
    let world = TestWorld::new();
    let env = world.env();
    let catalog = fixture_catalog();

    let fire = parse(&[
        "meta", "fire", "--prompt", "plan", "--repo", &repo.path().to_string_lossy(),
        "--name", "run-g", "--agent", "claude",
    ]);
    let meta_result = run_json(fire, &env, &catalog);
    let run_id = meta_result["run_id"].as_str().unwrap().to_string();

    let meta_env = world.env().with_var("OAT_RUN_ID", &run_id);
    let role_fire = parse(&["role", "fire", "worker", "--name", "w1", "--prompt", "do work"]);
    let role_result = run_json(role_fire, &meta_env, &catalog);
    let dispatch_id = role_result["dispatch_id"].as_str().unwrap().to_string();

    // The worker asks a question; this blocks, so it runs on its own thread while the
    // coordinator answers it with `run reply`.
    let worker_env_for_ask = world.env()
        .with_var("OAT_RUN_ID", &run_id)
        .with_var("OAT_DISPATCH_ID", &dispatch_id);
    let ask_cli = parse(&["dispatch", "ask", "--body", "which glaze?", "--timeout-ms", "5000"]);
    let catalog_for_thread = fixture_catalog();
    let ask_handle = std::thread::spawn(move || {
        execute(ask_cli, &worker_env_for_ask, &catalog_for_thread).unwrap()
    });

    // The coordinator receives the question through its own `run wait`, acknowledges the
    // delivery, and only then replies to it — the Run inbox is how it learns the message id.
    std::thread::sleep(std::time::Duration::from_millis(200));
    let wait_for_question = parse(&["run", "wait", "--run", &run_id, "--ack", "--timeout-ms", "5000"]);
    let question_delivery = run_json(wait_for_question, &meta_env, &catalog);
    assert_eq!(question_delivery["timed_out"], false);
    assert_eq!(question_delivery["messages"][0]["kind"], "Question");
    let question_seq = question_delivery["messages"][0]["seq"].as_u64().unwrap();

    let reply_cli = parse(&[
        "run", "reply", "--run", &run_id, "--message", &question_seq.to_string(), "--prompt", "cobalt",
    ]);
    run_json(reply_cli, &meta_env, &catalog);

    let ask_result = ask_handle.join().unwrap();
    assert_eq!(ask_result["timed_out"], false);
    assert_eq!(ask_result["reply"], "cobalt");

    // The worker settles.
    let worker_env = world.env()
        .with_var("OAT_RUN_ID", &run_id)
        .with_var("OAT_DISPATCH_ID", &dispatch_id);
    let done_cli = parse(&["dispatch", "done", "--report", "shipped the glaze"]);
    run_json(done_cli, &worker_env, &catalog);

    // The coordinator waits again, receives the worker_done delivery, and acknowledges it.
    let wait_cli = parse(&["run", "wait", "--run", &run_id, "--ack", "--timeout-ms", "5000"]);
    let wait_result = run_json(wait_cli, &meta_env, &catalog);
    assert_eq!(wait_result["timed_out"], false);
    assert_eq!(wait_result["messages"][0]["kind"], "WorkerDone");

    // The coordinator releases the Dispatch and its worktree, then finishes the Run.
    let release_cli = parse(&["dispatch", "release", "--dispatch", &dispatch_id, "--run", &run_id, "--remove-worktree", "--force"]);
    run_json(release_cli, &meta_env, &catalog);

    let finish_cli = parse(&["meta", "finish", "--run", &run_id]);
    let finish_result = run_json(finish_cli, &meta_env, &catalog);
    assert!(finish_result["closed_at"].is_string());

    let log_cli = parse(&["log", "show", "--run", &run_id, "--format", "json"]);
    let log_result = run_json(log_cli, &meta_env, &catalog);
    let events: Vec<String> = log_result["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["event"].as_str().unwrap().to_string())
        .collect();
    for expected in ["run_created", "agent_enter", "inbox_message", "inbox_reply", "agent_exit", "run_closed"] {
        assert!(events.contains(&expected.to_string()), "log is missing '{expected}': {events:?}");
    }
    let created_at = events.iter().position(|e| e == "run_created").unwrap();
    let closed_at = events.iter().position(|e| e == "run_closed").unwrap();
    assert!(created_at < closed_at, "run_created precedes run_closed");
}
