use crate::error::{codes, err, CliFailure};
use crate::role::{Backend, SkillRef};
use anyhow::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn write_skill_files(dir: &Path, skill: &SkillRef) -> Result<()> {
    for file in &skill.files {
        let path = dir.join(&file.relative_path);
        fs::create_dir_all(path.parent().unwrap())
            .map_err(|e| err(codes::SKILL_WRITE_FAILED, e.to_string()))?;
        fs::write(&path, &file.contents).map_err(|e| err(codes::SKILL_WRITE_FAILED, e.to_string()))?;
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
    for skill in skills {
        let relative_dir = format!("{root_rel}/{}", skill.name);
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
        write_skill_files(&dir, skill)?;
        append_local_exclude(worktree, &format!("{root_rel}/{}/", skill.name))?;
    }

    manifest.skills = declared;
    write_manifest(worktree, backend, &manifest)?;
    append_local_exclude(worktree, &format!("{root_rel}/.oat-manifest.json"))?;
    Ok(())
}
