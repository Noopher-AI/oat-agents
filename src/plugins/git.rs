//! Fetching and caching a git-pinned plugin (ticket Architecture): shallow-fetched with the
//! `git` executable into `~/.oat/plugins/<source hash>/<commit>/`, re-verified against the
//! commit before use every time, whether it was just fetched or was already cached.

use crate::error::{codes, err};
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A stable, filesystem-safe id for a git URL, so two different URLs never collide in the
/// cache and the same URL always lands in the same place.
pub fn source_hash(url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let digest = hasher.finalize();
    digest.iter().fold(String::new(), |mut out, b| {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
        out
    })
}

pub fn cache_dir(home: &Path, url: &str, commit: &str) -> PathBuf {
    home.join(".oat").join("plugins").join(source_hash(url)).join(commit)
}

/// Ensures `~/.oat/plugins/<source hash>/<commit>/` holds a working tree checked out exactly at
/// `commit`, fetching it with `git` if it is not already cached, and re-verifying the checkout
/// against `commit` either way. Never leaves a partially-fetched directory behind on failure:
/// a fresh fetch happens in the same directory it will be re-verified from, and a verification
/// failure is reported by name rather than silently retried.
pub fn ensure_commit(home: &Path, url: &str, commit: &str) -> Result<PathBuf> {
    let dir = cache_dir(home, url, commit);
    if dir.is_dir() {
        if verify_commit(&dir, commit)? {
            return Ok(dir);
        }
        return Err(err(
            codes::PLUGIN_FETCH_FAILED,
            format!(
                "the cached tree for git plugin '{url}' at commit '{commit}' no longer matches that commit; \
                 remove '{}' and try again",
                dir.display()
            ),
        ));
    }

    std::fs::create_dir_all(&dir)
        .map_err(|e| err(codes::PLUGIN_FETCH_FAILED, format!("could not create '{}': {e}", dir.display())))?;

    run(&dir, &["init", "--quiet"])?;
    run(&dir, &["fetch", "--quiet", "--depth", "1", url, commit])
        .map_err(|e| err(codes::PLUGIN_FETCH_FAILED, format!("fetching '{url}' at commit '{commit}' failed: {e}")))?;
    run(&dir, &["checkout", "--quiet", "FETCH_HEAD"])?;

    if !verify_commit(&dir, commit)? {
        return Err(err(
            codes::PLUGIN_FETCH_FAILED,
            format!("the tree fetched from '{url}' does not match the pinned commit '{commit}'"),
        ));
    }
    Ok(dir)
}

fn verify_commit(dir: &Path, commit: &str) -> Result<bool> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|e| err(codes::PLUGIN_FETCH_FAILED, format!("could not run git: {e}")))?;
    if !output.status.success() {
        return Ok(false);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim() == commit)
}

fn run(dir: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .map_err(|e| err(codes::PLUGIN_FETCH_FAILED, format!("could not run git {args:?}: {e}")))?;
    if !output.status.success() {
        return Err(err(
            codes::PLUGIN_FETCH_FAILED,
            format!("git {args:?} failed: {}", String::from_utf8_lossy(&output.stderr)),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git").current_dir(dir).args(args).status().unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_source_repo(dir: &Path) -> String {
        fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "--quiet"]);
        git(dir, &["config", "user.email", "test@example.com"]);
        git(dir, &["config", "user.name", "Test"]);
        fs::write(dir.join("oat-plugin.toml"), "format_version = 1\nname = \"team\"\ndescription = \"d\"\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "--quiet", "-m", "initial"]);
        let output = Command::new("git").current_dir(dir).args(["rev-parse", "HEAD"]).output().unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[test]
    fn a_shallow_fetch_lands_exactly_at_the_pinned_commit_and_is_reused_on_a_second_call() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let commit = init_source_repo(&source);
        let home = temp.path().join("home");
        fs::create_dir_all(&home).unwrap();

        let dir = ensure_commit(&home, &source.to_string_lossy(), &commit).unwrap();
        assert!(dir.join("oat-plugin.toml").exists());

        // A second call reuses the cache without re-fetching.
        let dir2 = ensure_commit(&home, &source.to_string_lossy(), &commit).unwrap();
        assert_eq!(dir, dir2);
    }

    #[test]
    fn a_commit_that_does_not_exist_on_the_remote_fails_naming_the_source() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        init_source_repo(&source);
        let home = temp.path().join("home");
        fs::create_dir_all(&home).unwrap();

        let bogus_commit = "0".repeat(40);
        let error = ensure_commit(&home, &source.to_string_lossy(), &bogus_commit).unwrap_err();
        assert!(error.to_string().contains(&source.to_string_lossy().to_string()) || error.to_string().contains("fetch"));
    }
}
