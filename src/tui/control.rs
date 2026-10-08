// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

//! The live view's only way to touch the machine: type into a session, capture its screen,
//! resize it, check whether it is alive, diff a worktree against its base, close or clean a
//! Run, and open or stop a repository's console. Rendering and key handling never call tmux
//! or git directly — only through this seam — so a test can run the whole view against a
//! recording stand-in.

use crate::environment::Environment;
use crate::session::tmux::Tmux;
use crate::store::Store;
use crate::tui::live::{LiveInput, ScreenMode};
use anyhow::Result;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// What one worktree changed, and the diff itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffReport {
    pub added: u32,
    pub removed: u32,
    pub text: String,
}

/// A repository's console (ADR-0005) as the live view shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleHandle {
    pub repo: PathBuf,
    pub backend: String,
    pub started_at: String,
    pub session: String,
    /// The console's own directory, where its backend keeps its transcript.
    pub dir: Option<PathBuf>,
}

pub trait Control {
    /// Closes a Run: the same lifecycle `meta finish` runs, callable by the operator directly
    /// when its own meta-agent cannot. Returns what happened, in a phrase.
    fn close_run(&self, run_id: &str) -> Result<String>;
    /// Removes the worktrees a finished Run left behind, keeping any with uncommitted work.
    /// Returns what happened, in a phrase.
    fn clean_run(&self, run_id: &str) -> Result<String>;
    /// Hands one key, or one paste, to that session as the terminal would.
    fn type_input(&self, session: &str, input: &LiveInput) -> Result<()>;
    /// Where the session's cursor is, when the program in it shows one.
    fn cursor(&self, session: &str) -> Option<(u16, u16)>;
    /// How the program in the session is drawing, for who scrolls it.
    fn screen_mode(&self, session: &str) -> Option<ScreenMode>;
    /// The session's screen, with its colours, as tmux draws it.
    fn capture(&self, session: &str) -> Result<String>;
    /// The screen and everything that scrolled off above it.
    fn capture_history(&self, session: &str) -> Result<String>;
    /// Pins the session's pane to a size, so a capture fits the box.
    fn resize(&self, session: &str, cols: u16, rows: u16) -> Result<()>;
    fn is_alive(&self, session: &str) -> bool;
    fn diff(&self, worktree: &Path, base: &str) -> Result<DiffReport>;
    /// Whether `repo`'s console has a live session to show right now.
    fn console_live(&self, repo: &Path) -> bool;
    /// Opens `repo`'s console, creating it on first use on the backend its plugins choose;
    /// reopening a live one is a no-op.
    fn console(&self, repo: &Path) -> Result<ConsoleHandle>;
    /// Stops `repo`'s console session. Returns what happened, in a phrase.
    fn stop_console(&self, repo: &Path) -> Result<String>;
    /// Puts text selected on a live screen on the operator's clipboard.
    fn copy(&self, text: &str) -> Result<()>;
}

pub struct RealControl<'a> {
    pub env: &'a dyn Environment,
}

impl RealControl<'_> {
    fn tmux(&self) -> Tmux {
        Tmux::from_env(self.env)
    }
}

impl Control for RealControl<'_> {
    fn close_run(&self, run_id: &str) -> Result<String> {
        crate::commands::meta::run(
            crate::commands::meta::MetaCommand::Finish(crate::commands::meta::FinishArgs {
                run: Some(run_id.to_string()),
            }),
            self.env,
            &crate::env::integration::LiveExecEnvironments,
        )?;
        Ok("closed".to_string())
    }

    fn clean_run(&self, run_id: &str) -> Result<String> {
        let store = Store::open(self.env)?;
        let cleaned = crate::commands::run::clean_run(self.env, &store, run_id, false)?;
        let dispatches = cleaned["dispatches"].as_array().cloned().unwrap_or_default();
        let removed = dispatches.iter().filter(|d| d["removed"] == Value::Bool(true)).count();
        let kept: Vec<String> = dispatches
            .iter()
            .filter_map(|d| d["kept_reason"].as_str().map(str::to_owned))
            .collect();
        Ok(match (removed, kept.len()) {
            (0, 0) => "nothing left to clean".to_string(),
            (_, 0) => format!("removed {removed} worktree{}", if removed == 1 { "" } else { "s" }),
            _ => format!("removed {removed}, kept {}: {}", kept.len(), kept.join("; ")),
        })
    }

    fn type_input(&self, session: &str, input: &LiveInput) -> Result<()> {
        let tmux = self.tmux();
        match input {
            LiveInput::Text(text) => tmux.send_text(session, text),
            LiveInput::Key(key) => tmux.send_key(session, key),
            LiveInput::Paste(text) => tmux.paste(session, text),
        }
    }

    fn cursor(&self, session: &str) -> Option<(u16, u16)> {
        self.tmux().cursor(session)
    }

    fn screen_mode(&self, session: &str) -> Option<ScreenMode> {
        self.tmux()
            .screen_mode(session)
            .map(|(alternate, mouse)| ScreenMode { alternate, mouse })
    }

    fn capture(&self, session: &str) -> Result<String> {
        self.tmux().capture(session)
    }

    fn capture_history(&self, session: &str) -> Result<String> {
        self.tmux().capture_history(session)
    }

    fn resize(&self, session: &str, cols: u16, rows: u16) -> Result<()> {
        self.tmux().resize_window(session, cols, rows)
    }

    fn is_alive(&self, session: &str) -> bool {
        self.tmux().session_alive(session).unwrap_or(false)
    }

    fn diff(&self, worktree: &Path, base: &str) -> Result<DiffReport> {
        let (added, removed) = crate::worktree::diff_stat(worktree, base)?;
        Ok(DiffReport {
            added,
            removed,
            text: crate::worktree::diff(worktree, base)?,
        })
    }

    fn console_live(&self, repo: &Path) -> bool {
        matches!(crate::console::status(self.env, repo), Ok(Some((_, true))))
    }

    fn console(&self, repo: &Path) -> Result<ConsoleHandle> {
        let record = crate::console::open(self.env, repo, None)?;
        let dir = repo
            .canonicalize()
            .ok()
            .and_then(|canonical| crate::console::console_dir(self.env, &canonical).ok());
        Ok(ConsoleHandle {
            repo: PathBuf::from(&record.repo),
            backend: record.backend,
            started_at: record.created_at,
            session: record.session,
            dir,
        })
    }

    fn stop_console(&self, repo: &Path) -> Result<String> {
        let record = crate::console::stop(self.env, repo)?;
        Ok(format!("stopped {}", record.session))
    }

    /// Through the terminal the view is drawn in (OSC 52), which reaches the operator's own
    /// clipboard even over ssh. Inside the operator's tmux, whose default is to ignore that
    /// from a program, tmux's own buffer is set as well, and tmux passes it on.
    fn copy(&self, text: &str) -> Result<()> {
        use std::io::Write;
        if self.env.var("TMUX").is_some_and(|tmux| !tmux.is_empty()) {
            let _ = load_operator_buffer(text);
        }
        let mut stdout = std::io::stdout();
        write!(stdout, "\x1b]52;c;{}\x07", base64(text.as_bytes()))?;
        stdout.flush()?;
        Ok(())
    }
}

/// `tmux load-buffer -w`, on the operator's own tmux (the one `$TMUX` names, not the private
/// server the agents run on): `-w` hands the buffer on to the terminal's clipboard too.
fn load_operator_buffer(text: &str) -> Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("tmux")
        .args(["load-buffer", "-w", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }
    child.wait()?;
    Ok(())
}

/// Standard base64 with padding, the form OSC 52 carries.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (at, byte)| n | ((*byte as u32) << (16 - 8 * at)));
        for at in 0..4 {
            if at <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * at)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
