mod common;

use common::TestWorld;
use oat_agents::role::memory::InMemoryCatalogBuilder;
use oat_agents::role::{Backend, CoreRole, CoreRoleDefinition, ModelSetting, SkillFile, SkillRef};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn skill_with_script() -> SkillRef {
    SkillRef {
        name: "team-skill".to_string(),
        files: vec![
            SkillFile {
                relative_path: PathBuf::from("SKILL.md"),
                contents: b"# Team skill".to_vec(),
                executable: false,
            },
            SkillFile {
                relative_path: PathBuf::from("check.sh"),
                contents: b"#!/bin/sh\necho ok\n".to_vec(),
                executable: true,
            },
        ],
    }
}

fn catalog_with_console_instructions(instructions: &str) -> oat_agents::role::memory::InMemoryCatalog {
    InMemoryCatalogBuilder::new()
        .with_core_role(
            CoreRole::Console,
            CoreRoleDefinition {
                instructions: instructions.to_string(),
                models: BTreeMap::from([(Backend::Claude, ModelSetting::default())]),
                skills: vec![skill_with_script()],
            },
        )
        .build()
}

#[test]
fn the_consoles_prompt_carries_the_catalogs_instructions_after_its_baseline() {
    let world = TestWorld::new();
    let env = world.env();
    let repo = tempfile::tempdir().unwrap();
    let catalog = catalog_with_console_instructions("Watch spend and flag anything over budget.");

    let record = oat_agents::console::open(&env, &catalog, repo.path(), Backend::Claude).unwrap();
    assert_eq!(record.backend, "claude");

    let dir = oat_agents::console::console_dir(&env, &repo.path().canonicalize().unwrap()).unwrap();
    let prompt = std::fs::read_to_string(dir.join("prompt.md")).unwrap();

    let baseline_at = prompt.find("Never consume a Run inbox").expect("baseline present");
    let instructions_at = prompt
        .find("Watch spend and flag anything over budget.")
        .expect("catalog instructions present");
    assert!(baseline_at < instructions_at, "instructions must follow the baseline");
}

#[test]
fn a_skill_with_a_script_lands_in_the_consoles_directory_with_the_script_executable() {
    let world = TestWorld::new();
    let env = world.env();
    let repo = tempfile::tempdir().unwrap();
    let catalog = catalog_with_console_instructions("team instructions");

    oat_agents::console::open(&env, &catalog, repo.path(), Backend::Claude).unwrap();

    let dir = oat_agents::console::console_dir(&env, &repo.path().canonicalize().unwrap()).unwrap();
    let script = dir.join(".claude/skills/team-skill/check.sh");
    assert!(script.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&script).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "the skill's script keeps its executable bit");
    }
}

#[test]
fn the_console_baseline_prohibits_the_inbox_release_teardown_and_interactive_panels() {
    let world = TestWorld::new();
    let env = world.env();
    let repo = tempfile::tempdir().unwrap();
    let catalog = catalog_with_console_instructions("team instructions");

    oat_agents::console::open(&env, &catalog, repo.path(), Backend::Claude).unwrap();
    let dir = oat_agents::console::console_dir(&env, &repo.path().canonicalize().unwrap()).unwrap();
    let prompt = std::fs::read_to_string(dir.join("prompt.md")).unwrap().to_lowercase();

    assert!(prompt.contains("never consume a run inbox"));
    assert!(prompt.contains("never release a dispatch"));
    assert!(prompt.contains("never fire a role"));
    assert!(prompt.contains("interactive panel"));
    for plugin_role in ["planner", "worker", "reviewer"] {
        assert!(!prompt.contains(plugin_role), "baseline must name no plugin role: {plugin_role}");
    }
}

#[test]
fn opening_a_console_delivers_the_core_system_view_skill_even_when_the_catalog_declares_none() {
    let world = TestWorld::new();
    let env = world.env();
    let repo = tempfile::tempdir().unwrap();
    let catalog = InMemoryCatalogBuilder::new()
        .with_core_role(
            CoreRole::Console,
            CoreRoleDefinition {
                instructions: "team instructions".to_string(),
                models: BTreeMap::from([(Backend::Claude, ModelSetting::default())]),
                skills: Vec::new(),
            },
        )
        .build();

    oat_agents::console::open(&env, &catalog, repo.path(), Backend::Claude).unwrap();

    let dir = oat_agents::console::console_dir(&env, &repo.path().canonicalize().unwrap()).unwrap();
    let skill_file = dir.join(".claude/skills/oat-system-view/SKILL.md");
    assert!(skill_file.exists(), "the core skill must be materialized even with no catalog skills");
    let contents = std::fs::read_to_string(&skill_file).unwrap();
    assert!(contents.contains("name: oat-system-view"));
}

#[test]
fn the_consoles_own_system_view_cross_reference_is_rewritten_per_backend() {
    let world = TestWorld::new();
    let env = world.env();
    let repo = tempfile::tempdir().unwrap();
    let catalog = catalog_with_console_instructions("team instructions");

    oat_agents::console::open(&env, &catalog, repo.path(), Backend::Claude).unwrap();
    let claude_dir = oat_agents::console::console_dir(&env, &repo.path().canonicalize().unwrap()).unwrap();
    let claude_prompt = std::fs::read_to_string(claude_dir.join("prompt.md")).unwrap();
    assert!(
        claude_prompt.contains("oat-system-view"),
        "the skill cross-reference is rewritten to a plain name for Claude Code: {claude_prompt}"
    );
    assert!(!claude_prompt.contains("$oat-system-view"));

    let world_codex = TestWorld::new();
    let env_codex = world_codex.env();
    let repo_codex = tempfile::tempdir().unwrap();
    oat_agents::console::open(&env_codex, &catalog, repo_codex.path(), Backend::Codex).unwrap();
    let codex_dir =
        oat_agents::console::console_dir(&env_codex, &repo_codex.path().canonicalize().unwrap()).unwrap();
    let codex_prompt = std::fs::read_to_string(codex_dir.join("prompt.md")).unwrap();
    assert!(
        codex_prompt.contains("$oat-system-view"),
        "the skill cross-reference is left as written for Codex: {codex_prompt}"
    );
}

#[test]
fn two_consoles_opened_from_two_repositories_get_two_directories() {
    let world = TestWorld::new();
    let env = world.env();
    let repo_a = tempfile::tempdir().unwrap();
    let repo_b = tempfile::tempdir().unwrap();
    let catalog = catalog_with_console_instructions("team instructions");

    oat_agents::console::open(&env, &catalog, repo_a.path(), Backend::Claude).unwrap();
    oat_agents::console::open(&env, &catalog, repo_b.path(), Backend::Claude).unwrap();

    let dir_a = oat_agents::console::console_dir(&env, &repo_a.path().canonicalize().unwrap()).unwrap();
    let dir_b = oat_agents::console::console_dir(&env, &repo_b.path().canonicalize().unwrap()).unwrap();

    assert_ne!(dir_a, dir_b);
    assert!(dir_a.exists());
    assert!(dir_b.exists());
}

#[test]
fn each_repositorys_read_only_run_listing_only_shows_its_own_runs() {
    use oat_agents::store::{RunRecord, Store};

    let world = TestWorld::new();
    let env = world.env();
    let repo_a = tempfile::tempdir().unwrap();
    let repo_b = tempfile::tempdir().unwrap();

    let store = Store::open(&env).unwrap();
    let base = RunRecord {
        id: "run-a".to_string(),
        name: "run-a".to_string(),
        repo: repo_a.path().canonicalize().unwrap().to_string_lossy().to_string(),
        base_branch: "main".to_string(),
        backend: "claude".to_string(),
        created_at: oat_agents::event_log::now_iso(),
        closed_at: None,
        plugins: Vec::new(),
        meta_worktree: None,
        meta_dispatch_id: None,
        big_plan: None,
    };
    store.create_run(&base).unwrap();
    let mut run_b = base.clone();
    run_b.id = "run-b".to_string();
    run_b.name = "run-b".to_string();
    run_b.repo = repo_b.path().canonicalize().unwrap().to_string_lossy().to_string();
    store.create_run(&run_b).unwrap();

    let catalog = InMemoryCatalogBuilder::new().build();

    let cli_a = oat_agents::Cli {
        command: oat_agents::TopCommand::Log {
            command: oat_agents::commands::log::LogCommand::Runs(oat_agents::commands::log::RunsArgs {
                repo: Some(repo_a.path().to_path_buf()),
            }),
        },
    };
    let result_a = oat_agents::execute(cli_a, &env, &catalog).unwrap();
    let runs_a = result_a["runs"].as_array().unwrap();
    assert_eq!(runs_a.len(), 1);
    assert_eq!(runs_a[0]["run_id"], "run-a");

    let cli_b = oat_agents::Cli {
        command: oat_agents::TopCommand::Log {
            command: oat_agents::commands::log::LogCommand::Runs(oat_agents::commands::log::RunsArgs {
                repo: Some(repo_b.path().to_path_buf()),
            }),
        },
    };
    let result_b = oat_agents::execute(cli_b, &env, &catalog).unwrap();
    let runs_b = result_b["runs"].as_array().unwrap();
    assert_eq!(runs_b.len(), 1);
    assert_eq!(runs_b[0]["run_id"], "run-b");
}
