// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

mod common;

use common::TempRepo;
use oat_agents::error::{codes, to_failure};
use oat_agents::launch::skills::materialize_skills;
use oat_agents::role::{Backend, SkillFile, SkillRef};
use std::path::PathBuf;
use std::process::Command;

fn skill(name: &str, files: Vec<SkillFile>) -> SkillRef {
    SkillRef {
        name: name.to_string(),
        files,
    }
}

fn file(relative: &str, contents: &str, executable: bool) -> SkillFile {
    SkillFile {
        relative_path: PathBuf::from(relative),
        contents: contents.as_bytes().to_vec(),
        executable,
    }
}

#[test]
fn a_launch_materializes_exactly_the_declared_skills_and_leaves_git_status_clean() {
    let repo = TempRepo::new();
    let path = repo.path();

    let skills = vec![
        skill("alpha", vec![file("SKILL.md", "# Alpha", false)]),
        skill("beta", vec![file("SKILL.md", "# Beta", false), file("run.sh", "#!/bin/sh\necho hi\n", true)]),
    ];
    materialize_skills(&path, Backend::Claude, &skills).unwrap();

    let alpha = path.join(".claude/skills/alpha/SKILL.md");
    let beta_script = path.join(".claude/skills/beta/run.sh");
    assert!(alpha.exists());
    assert!(beta_script.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&beta_script).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755, "an executable skill file keeps its mode");
    }

    let entries: Vec<_> = std::fs::read_dir(path.join(".claude/skills")).unwrap().collect();
    let names: Vec<String> = entries
        .into_iter()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n != ".oat-manifest.json")
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(sorted, vec!["alpha", "beta"]);

    let status = Command::new("git")
        .current_dir(&path)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    assert!(status.stdout.is_empty(), "git status is clean: {}", String::from_utf8_lossy(&status.stdout));

    let exclude = std::fs::read_to_string(path.join(".git/info/exclude")).unwrap();
    assert!(exclude.contains(".claude/skills/alpha/"));
    assert!(exclude.contains(".claude/skills/beta/"));
}

#[test]
fn a_second_launch_replaces_a_skill_this_role_no_longer_declares() {
    let repo = TempRepo::new();
    let path = repo.path();

    materialize_skills(&path, Backend::Claude, &[skill("alpha", vec![file("SKILL.md", "v1", false)])]).unwrap();
    assert!(path.join(".claude/skills/alpha").exists());

    materialize_skills(&path, Backend::Claude, &[skill("beta", vec![file("SKILL.md", "v1", false)])]).unwrap();
    assert!(!path.join(".claude/skills/alpha").exists(), "a skill no longer declared is removed");
    assert!(path.join(".claude/skills/beta").exists());
}

#[test]
fn a_repository_skill_of_the_same_name_refuses_the_launch_and_creates_nothing() {
    let repo = TempRepo::new();
    let path = repo.path();

    std::fs::create_dir_all(path.join(".claude/skills/taken")).unwrap();
    std::fs::write(path.join(".claude/skills/taken/SKILL.md"), "tracked").unwrap();
    Command::new("git").current_dir(&path).args(["add", "."]).status().unwrap();
    Command::new("git").current_dir(&path).args(["commit", "-q", "-m", "tracked skill"]).status().unwrap();

    let skills = vec![skill("taken", vec![file("SKILL.md", "plugin version", false)])];
    let err = materialize_skills(&path, Backend::Claude, &skills).unwrap_err();
    let failure = to_failure(&err);
    assert_eq!(failure.code, codes::SKILL_CONFLICT);
    assert!(failure.message.contains("taken"));
    assert!(failure.message.contains(".claude/skills/taken"));

    // Refusal creates nothing: the repository-tracked file is untouched.
    let content = std::fs::read_to_string(path.join(".claude/skills/taken/SKILL.md")).unwrap();
    assert_eq!(content, "tracked");
    assert!(!path.join(".claude/skills/.oat-manifest.json").exists());
}

#[test]
fn skill_md_rewrite_only_touches_a_declared_cross_reference_never_a_shell_variable() {
    let repo = TempRepo::new();
    let path = repo.path();

    let skill_md = "# Alpha\n\nSee $beta for details.\n\nExample: `echo $HOME/$PATH $1`\n";
    let skills = vec![
        skill("alpha", vec![file("SKILL.md", skill_md, false)]),
        skill("beta", vec![file("SKILL.md", "# Beta", false)]),
    ];

    materialize_skills(&path, Backend::Claude, &skills).unwrap();
    let claude_alpha = std::fs::read_to_string(path.join(".claude/skills/alpha/SKILL.md")).unwrap();
    assert!(claude_alpha.contains("See beta for details"), "the declared cross-reference is rewritten: {claude_alpha}");
    assert!(!claude_alpha.contains("$beta"));
    assert!(
        claude_alpha.contains("echo $HOME/$PATH $1"),
        "a shell-variable-shaped string is left untouched: {claude_alpha}"
    );

    let repo2 = TempRepo::new();
    let path2 = repo2.path();
    materialize_skills(&path2, Backend::Codex, &skills).unwrap();
    let codex_alpha = std::fs::read_to_string(path2.join(".agents/skills/alpha/SKILL.md")).unwrap();
    assert!(codex_alpha.contains("See $beta for details"), "Codex leaves the reference as written: {codex_alpha}");
    assert!(
        codex_alpha.contains("echo $HOME/$PATH $1"),
        "a shell-variable-shaped string is left untouched: {codex_alpha}"
    );
}

#[test]
fn codex_skills_land_under_agents_skills() {
    let repo = TempRepo::new();
    let path = repo.path();
    materialize_skills(&path, Backend::Codex, &[skill("alpha", vec![file("SKILL.md", "v1", false)])]).unwrap();
    assert!(path.join(".agents/skills/alpha/SKILL.md").exists());
    assert!(!path.join(".claude").exists());
}

/// A repository whose `.claude/skills` is a symlink to its tracked `.agents/skills`, so both
/// backends read one set of skills.
#[cfg(unix)]
fn repo_with_shared_skill_dir() -> TempRepo {
    let repo = TempRepo::new();
    let path = repo.path();
    std::fs::create_dir_all(path.join(".agents/skills/tdd")).unwrap();
    std::fs::write(path.join(".agents/skills/tdd/SKILL.md"), "# The repository's own").unwrap();
    std::fs::create_dir_all(path.join(".claude")).unwrap();
    std::os::unix::fs::symlink("../.agents/skills", path.join(".claude/skills")).unwrap();
    let git = |args: &[&str]| assert!(Command::new("git").current_dir(&path).args(args).status().unwrap().success());
    git(&["add", "-A"]);
    git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "shared skills"]);
    repo
}

#[cfg(unix)]
#[test]
fn skills_written_through_a_symlinked_skill_dir_leave_git_status_clean() {
    let repo = repo_with_shared_skill_dir();
    let path = repo.path();
    materialize_skills(&path, Backend::Claude, &[skill("alpha", vec![file("SKILL.md", "# Alpha", false)])]).unwrap();

    assert!(path.join(".agents/skills/alpha/SKILL.md").exists(), "written through the symlink");
    let status = Command::new("git").current_dir(&path).args(["status", "--porcelain"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&status.stdout), "", "nothing the launch wrote is visible to git");
}

#[cfg(unix)]
#[test]
fn a_tracked_skill_behind_a_symlinked_skill_dir_still_refuses_the_launch() {
    let repo = repo_with_shared_skill_dir();
    let path = repo.path();
    let error = materialize_skills(&path, Backend::Claude, &[skill("tdd", vec![file("SKILL.md", "plugin", false)])])
        .unwrap_err();
    assert_eq!(to_failure(&error).code, codes::SKILL_CONFLICT);
    assert_eq!(
        std::fs::read_to_string(path.join(".agents/skills/tdd/SKILL.md")).unwrap(),
        "# The repository's own",
        "the repository's skill is untouched"
    );
}
