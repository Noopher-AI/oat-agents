use crate::error::{codes, err};
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Lowercases, keeps `[a-z0-9-]`, collapses everything else to a single `-`, and trims the
/// result, so a run or launch name is always a safe git-branch and filesystem segment.
pub fn sanitize_segment(input: &str) -> String {
    let mut out = String::new();
    let mut last_was_dash = false;
    for ch in input.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_was_dash = false;
        } else if !last_was_dash {
            out.push('-');
            last_was_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "run".to_string()
    } else {
        trimmed
    }
}

/// `oat/<run-name>/meta` for the coordinator, `oat/<run-name>/<launch>` for a child.
pub fn branch_name(run_name: &str, launch: &str) -> String {
    format!("oat/{}/{}", sanitize_segment(run_name), sanitize_segment(launch))
}

/// `<repo-name>.oat-<run-name>-<launch>`, a sibling directory of the repository.
pub fn worktree_path(repo: &Path, run_name: &str, launch: &str) -> PathBuf {
    let repo_name = repo
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".to_string());
    let parent = repo.parent().unwrap_or(repo);
    parent.join(format!(
        "{repo_name}.oat-{}-{}",
        sanitize_segment(run_name),
        sanitize_segment(launch)
    ))
}

pub fn run_git(repo: &Path, args: &[&str]) -> Result<std::process::Output> {
    let output = Command::new("git").current_dir(repo).args(args).output()?;
    Ok(output)
}

/// Lines added and removed in `worktree` against `base`, summed over its files.
pub fn diff_stat(worktree: &Path, base: &str) -> Result<(u32, u32)> {
    let output = run_git(worktree, &["diff", "--numstat", base])?;
    if !output.status.success() {
        return Err(err(
            codes::INTERNAL_ERROR,
            format!("git diff failed: {}", String::from_utf8_lossy(&output.stderr).trim()),
        ));
    }
    let mut added = 0;
    let mut removed = 0;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut parts = line.split('\t');
        added += parts.next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
        removed += parts.next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
    }
    Ok((added, removed))
}

/// What `worktree` changed against `base`, as a unified diff.
pub fn diff(worktree: &Path, base: &str) -> Result<String> {
    let output = run_git(worktree, &["--no-pager", "diff", base])?;
    if !output.status.success() {
        return Err(err(
            codes::INTERNAL_ERROR,
            format!("git diff failed: {}", String::from_utf8_lossy(&output.stderr).trim()),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_ok(repo: &Path, args: &[&str]) -> Result<()> {
    let output = run_git(repo, args)?;
    if !output.status.success() {
        return Err(err(
            codes::INTERNAL_ERROR,
            format!(
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }
    Ok(())
}

/// Creates a worktree at `path`, on a fresh branch `branch` starting from `base_commit`.
pub fn create_worktree(repo: &Path, path: &Path, branch: &str, base_commit: &str) -> Result<()> {
    git_ok(
        repo,
        &[
            "worktree",
            "add",
            "-b",
            branch,
            &path.to_string_lossy(),
            base_commit,
        ],
    )
}

pub fn current_commit(repo: &Path) -> Result<String> {
    let output = run_git(repo, &["rev-parse", "HEAD"])?;
    if !output.status.success() {
        return Err(err(codes::INTERNAL_ERROR, "git rev-parse HEAD failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn default_branch(repo: &Path) -> Result<String> {
    let output = run_git(repo, &["symbolic-ref", "--short", "HEAD"])?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    Ok("main".to_string())
}

pub fn is_worktree_of(repo: &Path, path: &Path) -> Result<bool> {
    let output = run_git(repo, &["worktree", "list", "--porcelain"])?;
    let text = String::from_utf8_lossy(&output.stdout);
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    for line in text.lines() {
        if let Some(worktree_path) = line.strip_prefix("worktree ") {
            let candidate = PathBuf::from(worktree_path);
            let candidate = std::fs::canonicalize(&candidate).unwrap_or(candidate);
            if candidate == canonical {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub fn is_worktree_clean(path: &Path) -> Result<bool> {
    let output = run_git(path, &["status", "--porcelain"])?;
    Ok(output.status.success() && output.stdout.is_empty())
}

/// A branch is safe to delete only once the base already contains it — deleting it earlier
/// would lose commits the base never saw.
pub fn base_contains_branch(repo: &Path, base_branch: &str, branch: &str) -> Result<bool> {
    let output = run_git(
        repo,
        &["merge-base", "--is-ancestor", branch, base_branch],
    )?;
    Ok(output.status.success())
}

pub enum RemovalOutcome {
    Removed,
    Kept { reason: String },
}

/// Removes a Dispatch's worktree, and its branch when the base already contains it
/// (`run clean`'s terms, ticket §4.3). A dirty worktree is kept, with the reason recorded.
pub fn remove_worktree(
    repo: &Path,
    path: &Path,
    branch: &str,
    base_branch: &str,
    force: bool,
) -> Result<RemovalOutcome> {
    if !path.exists() {
        return Ok(RemovalOutcome::Removed);
    }
    if !force && !is_worktree_clean(path)? {
        return Ok(RemovalOutcome::Kept {
            reason: "worktree has uncommitted changes".to_string(),
        });
    }
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    let path_str = path.to_string_lossy().to_string();
    args.push(&path_str);
    git_ok(repo, &args)?;
    if base_contains_branch(repo, base_branch, branch)? {
        // Deleting the branch is best-effort: a branch already merged carries no risk, and a
        // failure here should not undo the worktree removal that already succeeded.
        let _ = run_git(repo, &["branch", "-D", branch]);
    }
    Ok(RemovalOutcome::Removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_segment_keeps_only_lowercase_alphanumerics_and_single_dashes() {
        assert_eq!(sanitize_segment("S1 Plugin-Based Core!!"), "s1-plugin-based-core");
        assert_eq!(sanitize_segment("  --leading and trailing--  "), "leading-and-trailing");
        assert_eq!(sanitize_segment(""), "run");
        assert_eq!(sanitize_segment("already-fine"), "already-fine");
    }

    #[test]
    fn branch_and_worktree_names_follow_the_spec_shape() {
        assert_eq!(branch_name("Glaze Run", "Meta"), "oat/glaze-run/meta");
        let repo = PathBuf::from("/home/me/code/oat-agents");
        let path = worktree_path(&repo, "glaze-run", "w1");
        assert_eq!(
            path,
            PathBuf::from("/home/me/code/oat-agents.oat-glaze-run-w1")
        );
    }
}
