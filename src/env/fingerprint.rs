//! What the code in a worktree looked like when a command ran.
//!
//! The ledger can already prove that `cargo test` ran in a particular image.
//! What it could not say is what it ran against, and without that a later role
//! has no way to tell a result it can rely on from one that describes code
//! nobody is reviewing any more. A fingerprint is that missing half: two
//! commands carrying the same one ran over byte-identical code.
//!
//! Everything here reads. No command written here creates a git object, moves
//! a ref, or stages anything; the worst it does is let git refresh its own
//! stat cache while answering a question.
//!
//! The one rule that governs every decision below: an answer this file is not
//! sure of is [`Fingerprint::Unknown`], and `Unknown` never matches anything,
//! including itself. Missing a chance to reuse a result costs a few minutes of
//! CPU. Wrongly calling code unchanged lets a review pass on evidence about
//! something else, and nothing downstream could tell.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// A content identity for a worktree, or an honest statement that there is none.
///
/// Deliberately not `PartialEq`: comparing two of these with `==` would make
/// `Unknown == Unknown` true, which is the one answer that must never be given.
/// Use [`same`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Fingerprint {
    Known {
        /// The full sha256. Callers truncate for display and never for
        /// comparison: a short hash that collides skips a verification in
        /// silence, which is the failure this module exists to prevent.
        digest: String,
        /// `None` on an unborn branch, where the index is the whole story.
        head: Option<String>,
        dirty: bool,
        untracked: usize,
    },
    Unknown {
        reason: UnknownReason,
    },
}

impl Fingerprint {
    pub fn digest(&self) -> Option<&str> {
        match self {
            Self::Known { digest, .. } => Some(digest),
            Self::Unknown { .. } => None,
        }
    }

    pub fn is_known(&self) -> bool {
        matches!(self, Self::Known { .. })
    }

    /// The first twelve hex digits, for a line a person reads. Never for a
    /// comparison.
    pub fn short(&self) -> String {
        match self {
            Self::Known { digest, .. } => digest.chars().take(12).collect(),
            Self::Unknown { reason } => format!("unknown ({})", reason.explain()),
        }
    }
}

/// Why there is no identity. Every one of these means "not reusable", and each
/// says something different to whoever is reading the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum UnknownReason {
    /// Nothing here is under version control, so there is nothing to compare.
    NotAGitWorktree,
    /// A submodule's own dirty state does not reach `git diff HEAD`, so the
    /// scope of what was hashed cannot be proven. Rather than hash most of the
    /// tree and imply it was all of it, this says so.
    Submodules,
    /// Hashing would have cost more than the answer is worth.
    TooLarge { files: usize, bytes: u64 },
    /// git refused. An index lock held by something else lands here, and so
    /// does an unborn branch with an unreadable index.
    GitFailed { status: Option<i32> },
    /// The budget ran out partway through.
    TimedOut,
}

impl UnknownReason {
    pub fn explain(&self) -> String {
        match self {
            Self::NotAGitWorktree => "not a git worktree".to_owned(),
            Self::Submodules => {
                "the repository has submodules, whose contents git diff does not report".to_owned()
            }
            Self::TooLarge { files, bytes } => {
                format!("{files} untracked files, {bytes} bytes, past the hashing budget")
            }
            Self::GitFailed { status } => match status {
                Some(code) => format!("git exited {code}"),
                None => "git could not be run".to_owned(),
            },
            Self::TimedOut => "git took longer than the budget allowed".to_owned(),
        }
    }
}

/// What this is allowed to spend before giving up and saying `Unknown`.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_untracked_files: usize,
    pub max_untracked_bytes: u64,
    /// Across every git call and every file read, not per call.
    pub budget: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_untracked_files: 5_000,
            max_untracked_bytes: 32 << 20,
            budget: Duration::from_secs(10),
        }
    }
}

/// True only when both sides are `Known` and identical.
///
/// Two `Unknown`s are never the same, however alike their reasons look. Not
/// knowing what the code was twice is not evidence that it did not change.
pub fn same(left: &Fingerprint, right: &Fingerprint) -> bool {
    match (left.digest(), right.digest()) {
        (Some(left), Some(right)) => left == right,
        // One side had no identity to compare. Not knowing what the code was
        // is not evidence that it stayed the same.
        _ => false,
    }
}

/// The identity of the code in `worktree`, as it stands.
pub fn of(worktree: &Path, limits: Limits) -> Fingerprint {
    let clock = Clock::new(limits.budget);
    let git = Git {
        worktree: worktree.to_path_buf(),
    };

    if let Err(reason) = clock.check() {
        return Fingerprint::Unknown { reason };
    }
    // `--git-dir` answers "is this version controlled at all" without caring
    // whether .git is a directory or the file a linked worktree leaves.
    if git.read(&["rev-parse", "--git-dir"]).is_none() {
        return Fingerprint::Unknown {
            reason: UnknownReason::NotAGitWorktree,
        };
    }
    // Checked before anything is hashed, because the answer would otherwise be
    // a digest over most of the tree presented as a digest over all of it.
    if worktree.join(".gitmodules").exists() {
        return Fingerprint::Unknown {
            reason: UnknownReason::Submodules,
        };
    }

    let mut hasher = Sha256::new();

    let head = git.read(&["rev-parse", "HEAD"]).map(|out| trimmed(&out));
    field(
        &mut hasher,
        b"head",
        head.as_deref().unwrap_or("").as_bytes(),
    );

    // With a HEAD, one diff covers staged and unstaged changes to tracked
    // files, including mode bits and binary content. Without one, the index is
    // the only record of what is tracked, so it is hashed directly.
    let tracked = match (&head, clock.check()) {
        (_, Err(reason)) => return Fingerprint::Unknown { reason },
        (Some(_), _) => git.read(&[
            "diff",
            "HEAD",
            "--no-color",
            // A repository that configures diff.external would otherwise make
            // the fingerprint a hash of some other program's output.
            "--no-ext-diff",
            "--ignore-submodules=none",
            "--binary",
        ]),
        (None, _) => match git.read(&["ls-files", "-s", "-z"]) {
            Some(mut index) => match git.read(&["diff", "--no-color", "--no-ext-diff", "--binary"])
            {
                Some(unstaged) => {
                    index.extend_from_slice(&unstaged);
                    Some(index)
                }
                None => None,
            },
            None => None,
        },
    };
    let Some(tracked) = tracked else {
        return Fingerprint::Unknown {
            reason: UnknownReason::GitFailed { status: None },
        };
    };
    let dirty = !tracked.is_empty();
    field(&mut hasher, b"tracked", &tracked);

    // What counts as untracked is decided by files outside the diff, so a
    // change to any of them changes the input to this hash without changing a
    // single line of code. They go in too.
    if let Err(reason) = clock.check() {
        return Fingerprint::Unknown { reason };
    }
    for source in exclude_sources(&git) {
        field(&mut hasher, b"exclude", &source);
    }

    let untracked = match untracked_paths(&git, &clock) {
        Ok(paths) => paths,
        Err(reason) => return Fingerprint::Unknown { reason },
    };
    if untracked.len() > limits.max_untracked_files {
        return Fingerprint::Unknown {
            reason: UnknownReason::TooLarge {
                files: untracked.len(),
                bytes: 0,
            },
        };
    }

    let mut bytes = 0u64;
    for relative in &untracked {
        if let Err(reason) = clock.check() {
            return Fingerprint::Unknown { reason };
        }
        let absolute = worktree.join(relative);
        field(
            &mut hasher,
            b"path",
            relative.as_os_str().as_encoded_bytes(),
        );
        // symlink_metadata, so a link pointing outside the worktree is
        // identified by where it points rather than followed into a file this
        // has no business reading.
        match std::fs::symlink_metadata(&absolute) {
            Ok(metadata) if metadata.is_symlink() => {
                let target = std::fs::read_link(&absolute).unwrap_or_default();
                field(
                    &mut hasher,
                    b"symlink",
                    target.as_os_str().as_encoded_bytes(),
                );
            }
            Ok(metadata) => {
                bytes = bytes.saturating_add(metadata.len());
                if bytes > limits.max_untracked_bytes {
                    return Fingerprint::Unknown {
                        reason: UnknownReason::TooLarge {
                            files: untracked.len(),
                            bytes,
                        },
                    };
                }
                match std::fs::read(&absolute) {
                    Ok(body) => field(&mut hasher, b"blob", &body),
                    // Raced with a delete, or unreadable. Either way this is no
                    // longer a tree anyone can describe.
                    Err(_) => {
                        return Fingerprint::Unknown {
                            reason: UnknownReason::GitFailed { status: None },
                        };
                    }
                }
            }
            // git listed it and it is gone: the tree moved while being read.
            Err(_) => {
                return Fingerprint::Unknown {
                    reason: UnknownReason::GitFailed { status: None },
                };
            }
        }
    }

    Fingerprint::Known {
        digest: format!("{:x}", hasher.finalize()),
        head,
        dirty: dirty || !untracked.is_empty(),
        untracked: untracked.len(),
    }
}

/// Length-prefixed, so no arrangement of contents can imitate another.
fn field(hasher: &mut Sha256, label: &[u8], body: &[u8]) {
    hasher.update(label);
    hasher.update([0]);
    hasher.update((body.len() as u64).to_le_bytes());
    hasher.update(body);
    hasher.update([0]);
}

fn trimmed(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_owned()
}

fn untracked_paths(git: &Git, clock: &Clock) -> Result<Vec<PathBuf>, UnknownReason> {
    clock.check()?;
    let listing = git
        .read(&["ls-files", "--others", "--exclude-standard", "-z"])
        .ok_or(UnknownReason::GitFailed { status: None })?;
    let mut paths: Vec<PathBuf> = listing
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| PathBuf::from(String::from_utf8_lossy(entry).into_owned()))
        .collect();
    paths.sort();
    Ok(paths)
}

/// The files that decide what "untracked" leaves out: the repository's own
/// `info/exclude` and whatever `core.excludesFile` points at. `.gitignore` is
/// tracked, so it is already in the diff.
fn exclude_sources(git: &Git) -> Vec<Vec<u8>> {
    let mut sources = Vec::new();
    if let Some(path) = git.read(&["rev-parse", "--git-path", "info/exclude"]) {
        let path = git.worktree.join(trimmed(&path));
        sources.push(std::fs::read(path).unwrap_or_default());
    }
    match git.read(&["config", "--get", "core.excludesFile"]) {
        Some(configured) if !trimmed(&configured).is_empty() => {
            let path = trimmed(&configured);
            sources.push(path.as_bytes().to_vec());
            sources.push(std::fs::read(expand_home(&path)).unwrap_or_default());
        }
        _ => sources.push(Vec::new()),
    }
    sources
}

fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => match std::env::var_os("HOME") {
            Some(home) => PathBuf::from(home).join(rest),
            None => PathBuf::from(path),
        },
        None => PathBuf::from(path),
    }
}

struct Clock {
    started: Instant,
    budget: Duration,
}

impl Clock {
    fn new(budget: Duration) -> Self {
        Self {
            started: Instant::now(),
            budget,
        }
    }

    fn check(&self) -> Result<(), UnknownReason> {
        if self.started.elapsed() > self.budget {
            return Err(UnknownReason::TimedOut);
        }
        Ok(())
    }
}

struct Git {
    worktree: PathBuf,
}

impl Git {
    /// stdout, or `None` when git could not be run or refused. Nothing here
    /// retries: a lock held by a command running in the pod is an answer, and
    /// waiting for it would spend the budget the caller set.
    fn read(&self, args: &[&str]) -> Option<Vec<u8>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.worktree)
            .args(args)
            .output()
            .ok()?;
        output.status.success().then_some(output.stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(worktree: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(worktree)
            .args([
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// A worktree with one tracked file and one commit.
    fn worktree() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path();
        git(path, &["init", "--quiet", "--initial-branch=main", "."]);
        std::fs::write(path.join("kept.txt"), "one\n").unwrap();
        git(path, &["add", "kept.txt"]);
        git(path, &["commit", "--quiet", "-m", "first"]);
        directory
    }

    fn digest(worktree: &Path) -> String {
        of(worktree, Limits::default())
            .digest()
            .expect("a git worktree has an identity")
            .to_owned()
    }

    fn reason(worktree: &Path, limits: Limits) -> UnknownReason {
        match of(worktree, limits) {
            Fingerprint::Unknown { reason } => reason,
            Fingerprint::Known { digest, .. } => {
                panic!("expected no identity, got {digest}")
            }
        }
    }

    #[test]
    fn the_same_code_hashes_the_same_way_twice() {
        let tree = worktree();
        assert_eq!(digest(tree.path()), digest(tree.path()));
    }

    #[test]
    fn editing_a_tracked_file_changes_it_and_putting_it_back_restores_it() {
        let tree = worktree();
        let before = digest(tree.path());

        std::fs::write(tree.path().join("kept.txt"), "two\n").unwrap();
        let edited = digest(tree.path());
        assert_ne!(before, edited, "an edit is a different tree");

        // The point of a content hash: the same bytes are the same tree, no
        // matter what happened in between.
        std::fs::write(tree.path().join("kept.txt"), "one\n").unwrap();
        assert_eq!(before, digest(tree.path()), "the edit was undone");
    }

    #[test]
    fn a_new_untracked_file_changes_it() {
        let tree = worktree();
        let before = digest(tree.path());
        std::fs::write(tree.path().join("new.rs"), "fn main() {}\n").unwrap();
        assert_ne!(before, digest(tree.path()));
    }

    #[test]
    fn an_ignored_file_does_not() {
        let tree = worktree();
        std::fs::write(tree.path().join(".gitignore"), "build/\n").unwrap();
        git(tree.path(), &["add", ".gitignore"]);
        git(tree.path(), &["commit", "--quiet", "-m", "ignore"]);
        let before = digest(tree.path());

        std::fs::create_dir(tree.path().join("build")).unwrap();
        std::fs::write(tree.path().join("build/out"), "artifact\n").unwrap();

        // Deliberate, and the known limit of this whole mechanism: what git is
        // told to ignore is invisible here. A build that depends on an ignored
        // file is a build this cannot describe, which is one reason a Reviewer
        // never loses the right to run a command again.
        assert_eq!(before, digest(tree.path()));
    }

    #[test]
    fn changing_gitignore_itself_does() {
        let tree = worktree();
        std::fs::write(tree.path().join(".gitignore"), "build/\n").unwrap();
        git(tree.path(), &["add", ".gitignore"]);
        git(tree.path(), &["commit", "--quiet", "-m", "ignore"]);
        let before = digest(tree.path());

        std::fs::write(tree.path().join(".gitignore"), "build/\ntmp/\n").unwrap();
        assert_ne!(
            before,
            digest(tree.path()),
            "what is ignored decides what is hashed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn making_a_tracked_file_executable_does() {
        use std::os::unix::fs::PermissionsExt;
        let tree = worktree();
        let before = digest(tree.path());

        let file = tree.path().join("kept.txt");
        let mut permissions = std::fs::metadata(&file).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&file, permissions).unwrap();

        assert_ne!(before, digest(tree.path()), "git records the exec bit");
    }

    #[cfg(unix)]
    #[test]
    fn an_untracked_symlink_is_read_as_its_target_not_followed() {
        let tree = worktree();
        let before = digest(tree.path());

        // Pointing outside the worktree, at something that does not exist:
        // following it would either read a file this has no business reading
        // or fail outright. Neither happens.
        std::os::unix::fs::symlink("/nonexistent/elsewhere", tree.path().join("link")).unwrap();
        let linked = digest(tree.path());
        assert_ne!(before, linked);

        std::fs::remove_file(tree.path().join("link")).unwrap();
        std::os::unix::fs::symlink("/nonexistent/other", tree.path().join("link")).unwrap();
        assert_ne!(
            linked,
            digest(tree.path()),
            "where it points is what identifies it"
        );
    }

    #[test]
    fn a_directory_under_no_version_control_has_no_identity_rather_than_an_empty_one() {
        let plain = tempfile::tempdir().unwrap();
        let fingerprint = of(plain.path(), Limits::default());
        assert!(matches!(
            fingerprint,
            Fingerprint::Unknown {
                reason: UnknownReason::NotAGitWorktree
            }
        ));
        assert_eq!(fingerprint.digest(), None);
        // The failure worth guarding: an unknown tree must not compare equal
        // to a known one, whatever the known one is.
        let tree = worktree();
        assert!(!same(&fingerprint, &of(tree.path(), Limits::default())));
    }

    #[test]
    fn a_tree_too_big_to_hash_is_unknown_rather_than_guessed_at() {
        let tree = worktree();
        for index in 0..4 {
            std::fs::write(tree.path().join(format!("extra-{index}")), "x").unwrap();
        }
        let limits = Limits {
            max_untracked_files: 2,
            ..Limits::default()
        };
        assert!(matches!(
            reason(tree.path(), limits),
            UnknownReason::TooLarge { files: 4, .. }
        ));

        let limits = Limits {
            max_untracked_bytes: 0,
            ..Limits::default()
        };
        assert!(matches!(
            reason(tree.path(), limits),
            UnknownReason::TooLarge { .. }
        ));
    }

    #[test]
    fn a_repository_with_submodules_cannot_prove_what_it_covered() {
        let tree = worktree();
        std::fs::write(
            tree.path().join(".gitmodules"),
            "[submodule \"vendor\"]\n\tpath = vendor\n\turl = ../vendor.git\n",
        )
        .unwrap();
        assert_eq!(
            reason(tree.path(), Limits::default()),
            UnknownReason::Submodules
        );
    }

    #[test]
    fn two_unknown_trees_are_never_the_same_tree() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let left = of(first.path(), Limits::default());
        let right = of(second.path(), Limits::default());

        // Both are `NotAGitWorktree`, so the reasons match. Not knowing what
        // the code was, twice, says nothing about whether it changed.
        assert!(!same(&left, &right));
        assert!(!same(&left, &left));
    }

    #[test]
    fn a_known_tree_is_the_same_as_itself() {
        let tree = worktree();
        let taken = of(tree.path(), Limits::default());
        assert!(same(&taken, &of(tree.path(), Limits::default())));
        assert!(taken.is_known());
        assert_eq!(taken.short().len(), 12);
    }

    #[test]
    fn a_dirty_tree_says_so_and_counts_what_is_not_tracked() {
        let tree = worktree();
        match of(tree.path(), Limits::default()) {
            Fingerprint::Known {
                dirty, untracked, ..
            } => {
                assert!(!dirty);
                assert_eq!(untracked, 0);
            }
            Fingerprint::Unknown { reason } => panic!("{}", reason.explain()),
        }

        std::fs::write(tree.path().join("kept.txt"), "changed\n").unwrap();
        std::fs::write(tree.path().join("loose"), "new\n").unwrap();
        match of(tree.path(), Limits::default()) {
            Fingerprint::Known {
                dirty,
                untracked,
                head,
                ..
            } => {
                assert!(dirty);
                assert_eq!(untracked, 1);
                assert_eq!(head.map(|id| id.len()), Some(40));
            }
            Fingerprint::Unknown { reason } => panic!("{}", reason.explain()),
        }
    }

    #[test]
    fn an_exhausted_budget_is_a_timeout_rather_than_a_partial_answer() {
        let tree = worktree();
        let limits = Limits {
            budget: Duration::ZERO,
            ..Limits::default()
        };
        // The first check is before any git call, so a zero budget can only
        // produce a timeout — never a digest over nothing.
        assert!(matches!(
            reason(tree.path(), limits),
            UnknownReason::TimedOut
        ));
    }
}
