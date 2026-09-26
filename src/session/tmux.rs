use crate::environment::Environment;
use crate::error::{codes, err};
use anyhow::Result;
use std::path::Path;
use std::process::Command;

/// The oat-agents private tmux server: executable and socket are overridable so tests can
/// substitute a recording script for a real tmux (ticket §7.1).
pub struct Tmux {
    command: String,
    socket: String,
}

impl Tmux {
    pub fn from_env(env: &dyn Environment) -> Self {
        Self {
            command: env.var_or("OAT_TMUX_COMMAND", "tmux"),
            socket: env.var_or("OAT_TMUX_SOCKET", "oat"),
        }
    }

    fn base_args(&self) -> Vec<String> {
        vec!["-L".to_string(), self.socket.clone()]
    }

    fn run(&self, args: &[String]) -> Result<std::process::Output> {
        Command::new(&self.command)
            .args(args)
            .output()
            .map_err(|e| {
                err(
                    codes::TMUX_MISSING,
                    format!("could not run '{}': {e}", self.command),
                )
            })
    }

    pub fn check_available(&self) -> Result<()> {
        let mut args = self.base_args();
        args.push("list-sessions".to_string());
        // A private server with no sessions yet exits nonzero with a "no server" message,
        // which is not a missing tmux; only a failure to exec the binary at all is fatal here.
        self.run(&args).map(|_| ())
    }

    /// `oat_<hash_id>` with `.` and `:` replaced by `_`, since tmux treats both as separators.
    pub fn session_name(hash_id: &str) -> String {
        format!("oat_{}", hash_id.replace(['.', ':'], "_"))
    }

    /// The exact argv `new-session` is invoked with, exposed so a test can assert on it
    /// (ticket §7.1) without starting anything real.
    pub fn launch_argv(
        &self,
        session: &str,
        cwd: &Path,
        env_vars: &[(String, String)],
        command: &str,
    ) -> Vec<String> {
        let mut args = self.base_args();
        args.push("new-session".to_string());
        args.push("-d".to_string());
        args.push("-s".to_string());
        args.push(session.to_string());
        args.push("-c".to_string());
        args.push(cwd.to_string_lossy().to_string());
        for (k, v) in env_vars {
            args.push("-e".to_string());
            args.push(format!("{k}={v}"));
        }
        args.push("--".to_string());
        args.push(command.to_string());
        args
    }

    pub fn start_session(
        &self,
        session: &str,
        cwd: &Path,
        env_vars: &[(String, String)],
        command: &str,
    ) -> Result<()> {
        let args = self.launch_argv(session, cwd, env_vars, command);
        let output = self.run(&args)?;
        if !output.status.success() {
            return Err(err(
                codes::INTERNAL_ERROR,
                format!(
                    "tmux new-session failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            ));
        }
        Ok(())
    }

    pub fn session_alive(&self, session: &str) -> Result<bool> {
        let mut args = self.base_args();
        args.push("has-session".to_string());
        args.push("-t".to_string());
        args.push(session.to_string());
        let output = self.run(&args)?;
        Ok(output.status.success())
    }

    pub fn kill_session(&self, session: &str) -> Result<()> {
        let mut args = self.base_args();
        args.push("kill-session".to_string());
        args.push("-t".to_string());
        args.push(session.to_string());
        let _ = self.run(&args)?;
        Ok(())
    }

    /// The visible screen of a session, for `dispatch read --source terminal`.
    pub fn capture_pane(&self, session: &str) -> Result<String> {
        let mut args = self.base_args();
        args.push("capture-pane".to_string());
        args.push("-p".to_string());
        args.push("-t".to_string());
        args.push(session.to_string());
        let output = self.run(&args)?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}
