//! The live view's only way to touch the machine (ticket Architecture): type into a session,
//! capture its screen, resize it, check whether it is alive, and diff a worktree against its
//! base branch. Rendering and key handling never call tmux or git directly — only through this
//! seam — so a test can run the whole view against a recording stand-in.

use crate::environment::Environment;
use crate::session::tmux::Tmux;
use anyhow::Result;
use std::path::Path;

pub trait Control {
    fn session_alive(&self, session: &str) -> bool;
    fn capture_pane(&self, session: &str) -> Result<String>;
    fn send_text(&self, session: &str, text: &str) -> Result<()>;
    fn send_key(&self, session: &str, key: &str) -> Result<()>;
    fn resize(&self, session: &str, cols: u16, rows: u16) -> Result<()>;
    fn diff_worktree(&self, worktree: &Path, base_branch: &str) -> Result<String>;
    /// Closes a Run: the same lifecycle `meta finish` runs, callable by the operator directly
    /// when its own coordinator cannot (ticket Scope: "closing a Run").
    fn close_run(&self, run_id: &str) -> Result<()>;
}

pub struct RealControl<'a> {
    pub env: &'a dyn Environment,
}

impl Control for RealControl<'_> {
    fn session_alive(&self, session: &str) -> bool {
        Tmux::from_env(self.env).session_alive(session).unwrap_or(false)
    }

    fn capture_pane(&self, session: &str) -> Result<String> {
        Tmux::from_env(self.env).capture_pane(session)
    }

    fn send_text(&self, session: &str, text: &str) -> Result<()> {
        Tmux::from_env(self.env).send_text(session, text)
    }

    fn send_key(&self, session: &str, key: &str) -> Result<()> {
        Tmux::from_env(self.env).send_key(session, key)
    }

    fn resize(&self, session: &str, cols: u16, rows: u16) -> Result<()> {
        Tmux::from_env(self.env).resize_window(session, cols, rows)
    }

    fn diff_worktree(&self, worktree: &Path, base_branch: &str) -> Result<String> {
        let output = crate::worktree::run_git(worktree, &["diff", base_branch])?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn close_run(&self, run_id: &str) -> Result<()> {
        // `meta finish` takes a catalog only because every top command does; closing a Run
        // never reads it, so an empty one stands in here.
        let catalog = crate::role::memory::InMemoryCatalogBuilder::new().build();
        crate::commands::meta::run(
            crate::commands::meta::MetaCommand::Finish(crate::commands::meta::FinishArgs {
                run: Some(run_id.to_string()),
            }),
            self.env,
            &catalog,
        )?;
        Ok(())
    }
}
