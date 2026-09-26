#![allow(dead_code)]

use oat_agents::environment::TestEnvironment;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A temporary git repository with one commit on `main`, ready to be a Run's repository.
pub struct TempRepo {
    pub dir: tempfile::TempDir,
}

impl TempRepo {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("repo");
        std::fs::create_dir_all(&path).unwrap();
        run(&path, &["init", "-q", "-b", "main"]);
        run(&path, &["config", "user.email", "test@example.com"]);
        run(&path, &["config", "user.name", "Test"]);
        std::fs::write(path.join("README.md"), "hello\n").unwrap();
        run(&path, &["add", "."]);
        run(&path, &["commit", "-q", "-m", "initial"]);
        Self { dir }
    }

    pub fn path(&self) -> PathBuf {
        self.dir.path().join("repo")
    }
}

fn run(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

/// Writes a recording tmux stand-in (ticket §7.1): it appends its whole argv to `log_path` and
/// exits 0. No test in this crate needs a real model or a real tmux server.
pub fn tmux_stub(scratch: &Path, log_path: &Path) -> PathBuf {
    let script = scratch.join("fake-tmux.sh");
    let contents = format!(
        "#!/bin/sh\numask 022\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
        log_path.display()
    );
    std::fs::write(&script, contents).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    script
}

pub struct TestWorld {
    pub scratch: tempfile::TempDir,
    pub home: PathBuf,
    pub tmux_log: PathBuf,
}

impl TestWorld {
    pub fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let home = scratch.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let tmux_log = scratch.path().join("tmux.log");
        Self {
            scratch,
            home,
            tmux_log,
        }
    }

    pub fn env(&self) -> TestEnvironment {
        let tmux = tmux_stub(self.scratch.path(), &self.tmux_log);
        TestEnvironment::new(self.home.clone())
            .with_var("OAT_TMUX_COMMAND", tmux.to_string_lossy().to_string())
            .with_var("OAT_TMUX_SOCKET", "oat-test")
            .with_var(
                "OAT_AGENTS_STATE_DIR",
                self.scratch.path().join("state").to_string_lossy().to_string(),
            )
    }

    pub fn tmux_calls(&self) -> Vec<String> {
        std::fs::read_to_string(&self.tmux_log)
            .unwrap_or_default()
            .lines()
            .map(|s| s.to_string())
            .collect()
    }
}
