// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

use std::collections::HashMap;
use std::path::PathBuf;

/// Everything the crate reads from the outside world that a test must be able to fake:
/// environment variables and the user's home directory. Every command takes `&dyn Environment`
/// instead of calling `std::env` directly, so a test can run the real CLI logic against a
/// temporary, disposable world.
pub trait Environment {
    fn var(&self, key: &str) -> Option<String>;
    fn home_dir(&self) -> Option<PathBuf>;

    fn var_or(&self, key: &str, default: &str) -> String {
        self.var(key).unwrap_or_else(|| default.to_string())
    }
}

/// The real environment: `std::env` and the OS home directory.
pub struct SystemEnvironment;

impl Environment for SystemEnvironment {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn home_dir(&self) -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// A fully in-memory environment for tests: an explicit variable map and an explicit home,
/// usually a `tempfile::tempdir()`.
pub struct TestEnvironment {
    vars: HashMap<String, String>,
    home: Option<PathBuf>,
}

impl TestEnvironment {
    pub fn new(home: PathBuf) -> Self {
        Self {
            vars: HashMap::new(),
            home: Some(home),
        }
    }

    pub fn with_var(mut self, key: &str, value: impl Into<String>) -> Self {
        self.vars.insert(key.to_string(), value.into());
        self
    }
}

impl Environment for TestEnvironment {
    fn var(&self, key: &str) -> Option<String> {
        self.vars.get(key).cloned()
    }

    fn home_dir(&self) -> Option<PathBuf> {
        self.home.clone()
    }
}

/// Resolution order for the workflow-log directory (ticket §4.1):
/// `OAT_AGENTS_LOG_DIR`, else `$XDG_STATE_HOME/oat-agents/log`, else
/// `~/.local/state/oat-agents/log`. No home directory means logging is disabled, never fatal.
pub fn log_dir(env: &dyn Environment) -> Option<PathBuf> {
    if let Some(dir) = env.var("OAT_AGENTS_LOG_DIR") {
        return Some(PathBuf::from(dir));
    }
    if let Some(xdg) = env.var("XDG_STATE_HOME") {
        return Some(PathBuf::from(xdg).join("oat-agents").join("log"));
    }
    env.home_dir()
        .map(|home| home.join(".local/state/oat-agents/log"))
}

/// Resolution order for the Run store root (`runs/`): `OAT_AGENTS_STATE_DIR/runs`, else beside
/// the log directory, else `$XDG_STATE_HOME/oat-agents/runs`, else under the home directory.
pub fn store_root(env: &dyn Environment) -> Option<PathBuf> {
    if let Some(dir) = env.var("OAT_AGENTS_STATE_DIR") {
        return Some(PathBuf::from(dir).join("runs"));
    }
    if let Some(dir) = env.var("OAT_AGENTS_LOG_DIR") {
        return PathBuf::from(dir).parent().map(|p| p.join("runs"));
    }
    if let Some(xdg) = env.var("XDG_STATE_HOME") {
        return Some(PathBuf::from(xdg).join("oat-agents").join("runs"));
    }
    env.home_dir()
        .map(|home| home.join(".local/state/oat-agents/runs"))
}

/// Resolution order for the checklist directory: `OAT_AGENTS_CHECKLIST_DIR`, else beside the
/// store root.
pub fn checklist_dir(env: &dyn Environment) -> Option<PathBuf> {
    if let Some(dir) = env.var("OAT_AGENTS_CHECKLIST_DIR") {
        return Some(PathBuf::from(dir));
    }
    store_root(env).and_then(|root| root.parent().map(|p| p.join("checklists")))
}
