use crate::error::{codes, err, CliFailure};
use crate::launch::prompt;
use crate::role::{Backend, SkillFile, SkillRef};
use anyhow::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The core skill for reading the system (ADR-0004): the CLI supplies it, no plugin decides
/// whether it exists, so every place that materializes an agent's skills adds it alongside
/// whatever the catalog declares, rather than the catalog declaring it itself.
pub fn oat_system_view_skill() -> SkillRef {
    SkillRef {
        name: "oat-system-view".to_string(),
        files: vec![SkillFile {
            relative_path: PathBuf::from("SKILL.md"),
            contents: prompt::OAT_SYSTEM_VIEW_SKILL.as_bytes().to_vec(),
            executable: false,
        }],
    }
}

/// The worktree-relative directory a backend reads project-level skills from (ADR-0003).
/// Verified on this machine against an installed Codex (0.155.1): its own `--help`/`doctor`
/// output names no project-level skill path, but every existing `.agents/`-based project on
/// this machine keeps its skills at `.agents/skills/<name>/`, so that is the convention this
/// crate targets; see the pull request for the note this ticket requires.
fn skill_root(backend: Backend) -> &'static str {
    match backend {
        Backend::Claude => ".claude/skills",
        Backend::Codex => ".agents/skills",
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Manifest {
    skills: BTreeSet<String>,
}

/// The skill directory as git sees it, relative to the worktree root. A repository may make
/// the backend's directory a symlink — `.claude/skills -> ../.agents/skills` so both backends
/// read the same skills — and then files written under `.claude/skills/` land, for git, under
/// `.agents/skills/`. Conflict checks and exclude entries must name that path, or a tracked
/// skill is overwritten unnoticed and the written skills show up as untracked files.
fn repo_skill_root(worktree: &Path, backend: Backend) -> Result<String> {
    let skill_root = skill_root(backend);
    let root = worktree
        .canonicalize()
        .map_err(|e| err(codes::SKILL_WRITE_FAILED, format!("cannot resolve {}: {e}", worktree.display())))?;
    let mut existing = worktree.join(skill_root);
    let mut missing = Vec::new();
    while fs::symlink_metadata(&existing).is_err() {
        let Some(name) = existing.file_name() else { break };
        missing.push(name.to_owned());
        existing.pop();
    }
    let mut resolved = existing.canonicalize().map_err(|e| {
        err(codes::SKILL_WRITE_FAILED, format!("cannot resolve {}: {e}", existing.display()))
    })?;
    resolved.extend(missing.iter().rev());
    let relative = resolved.strip_prefix(&root).map_err(|_| {
        err(
            codes::SKILL_WRITE_FAILED,
            format!(
                "'{skill_root}' resolves to '{}', outside the worktree; skills are written only into it (ADR-0003)",
                resolved.display()
            ),
        )
    })?;
    Ok(relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

fn manifest_path(worktree: &Path, backend: Backend) -> PathBuf {
    worktree.join(skill_root(backend)).join(".oat-manifest.json")
}

fn read_manifest(worktree: &Path, backend: Backend) -> Manifest {
    let path = manifest_path(worktree, backend);
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_manifest(worktree: &Path, backend: Backend, manifest: &Manifest) -> Result<()> {
    let path = manifest_path(worktree, backend);
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, serde_json::to_string_pretty(manifest)?)?;
    Ok(())
}

fn git_common_dir(worktree: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .current_dir(worktree)
        .args(["rev-parse", "--git-common-dir"])
        .output()?;
    if !output.status.success() {
        return Err(err(codes::INTERNAL_ERROR, "git rev-parse --git-common-dir failed"));
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let path = PathBuf::from(text);
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(worktree.join(path))
    }
}

fn tracked_files_under(worktree: &Path, relative_dir: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .current_dir(worktree)
        .args(["ls-files", "--", relative_dir])
        .output()?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

pub fn append_local_exclude(worktree: &Path, entry: &str) -> Result<()> {
    let common_dir = git_common_dir(worktree)?;
    let exclude_path = common_dir.join("info").join("exclude");
    fs::create_dir_all(exclude_path.parent().unwrap())?;
    let existing = fs::read_to_string(&exclude_path).unwrap_or_default();
    if existing.lines().any(|l| l == entry) {
        return Ok(());
    }
    let mut content = existing;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(entry);
    content.push('\n');
    fs::write(exclude_path, content)?;
    Ok(())
}

/// Writes a skill's files, rewriting `$skill-name` cross-references (ADR-0002) in `SKILL.md`
/// only — never in a script or other supporting file, which are not prose and may legitimately
/// contain a literal `$` followed by word characters (a shell variable, for instance).
fn write_skill_files(dir: &Path, skill: &SkillRef, backend: Backend, known_skills: &BTreeSet<String>) -> Result<()> {
    for file in &skill.files {
        let path = dir.join(&file.relative_path);
        fs::create_dir_all(path.parent().unwrap())
            .map_err(|e| err(codes::SKILL_WRITE_FAILED, e.to_string()))?;
        let contents = if file.relative_path == Path::new("SKILL.md") {
            match std::str::from_utf8(&file.contents) {
                Ok(text) => {
                    crate::launch::prompt::rewrite_skill_references(text, backend, known_skills).into_bytes()
                }
                Err(_) => file.contents.clone(),
            }
        } else {
            file.contents.clone()
        };
        fs::write(&path, &contents).map_err(|e| err(codes::SKILL_WRITE_FAILED, e.to_string()))?;
        #[cfg(unix)]
        if file.executable {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                .map_err(|e| err(codes::SKILL_WRITE_FAILED, e.to_string()))?;
        }
    }
    Ok(())
}

/// Writes exactly the skills a role declares into the Dispatch's own worktree, refusing when
/// the repository already tracks a project-level skill of the same name (ADR-0003). Nothing is
/// created if any declared skill conflicts — this must run to completion, or not at all,
/// before a launch proceeds (ticket §6.2 rule 3).
pub fn materialize_skills(worktree: &Path, backend: Backend, skills: &[SkillRef]) -> Result<()> {
    let root_rel = skill_root(backend);
    let repo_rel = repo_skill_root(worktree, backend)?;
    for skill in skills {
        let relative_dir = format!("{repo_rel}/{}", skill.name);
        let tracked = tracked_files_under(worktree, &relative_dir)?;
        if !tracked.is_empty() {
            return Err(Error::new(
                CliFailure::new(
                    codes::SKILL_CONFLICT,
                    format!(
                        "skill '{}' conflicts with a repository-tracked path '{}'",
                        skill.name, relative_dir
                    ),
                )
                .with_details(serde_json::json!({
                    "skill": skill.name,
                    "path": relative_dir,
                    "tracked_files": tracked,
                })),
            ));
        }
    }

    let mut manifest = read_manifest(worktree, backend);
    let declared: BTreeSet<String> = skills.iter().map(|s| s.name.clone()).collect();

    // Remove a skill this launch's role no longer declares, but that an earlier launch in
    // this worktree wrote (an `existing`-start role reusing another Dispatch's worktree).
    for stale in manifest.skills.difference(&declared).cloned().collect::<Vec<_>>() {
        let dir = worktree.join(root_rel).join(&stale);
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|e| err(codes::SKILL_WRITE_FAILED, e.to_string()))?;
        }
    }

    for skill in skills {
        let dir = worktree.join(root_rel).join(&skill.name);
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|e| err(codes::SKILL_WRITE_FAILED, e.to_string()))?;
        }
        write_skill_files(&dir, skill, backend, &declared)?;
        append_local_exclude(worktree, &format!("{repo_rel}/{}/", skill.name))?;
    }

    manifest.skills = declared;
    write_manifest(worktree, backend, &manifest)?;
    append_local_exclude(worktree, &format!("{repo_rel}/.oat-manifest.json"))?;
    Ok(())
}

/// Writes a role's skills under a plain directory that is not a git worktree — the console's
/// own directory (ADR-0003's "keeps its skills in its own directory under `~/.oat/`"). There is
/// no repository to conflict with and no local exclude file to update, so this skips both.
pub fn materialize_skills_plain(dir: &Path, backend: Backend, skills: &[SkillRef]) -> Result<()> {
    let root_rel = skill_root(backend);
    let known_skills: BTreeSet<String> = skills.iter().map(|s| s.name.clone()).collect();
    for skill in skills {
        let skill_dir = dir.join(root_rel).join(&skill.name);
        if skill_dir.exists() {
            fs::remove_dir_all(&skill_dir).map_err(|e| err(codes::SKILL_WRITE_FAILED, e.to_string()))?;
        }
        write_skill_files(&skill_dir, skill, backend, &known_skills)?;
    }
    Ok(())
}
