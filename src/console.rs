//! `oat-console` (ADR-0005): one per initialised repository, launched in its own directory
//! outside the repository and outside any Run, and outliving any single launch of it.

use crate::environment::Environment;
use crate::error::{codes, err};
use crate::event_log::now_iso;
use crate::launch::{self, prompt, skills};
use crate::role::{Backend, CoreRole, RoleCatalog};
use crate::session::tmux::Tmux;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleRecord {
    pub repo: String,
    pub backend: String,
    pub created_at: String,
    pub session: String,
}

/// A stable, filesystem-safe id for a repository, derived from its canonical path (ticket
/// Contracts). Two distinct repositories always get two distinct ids and therefore two
/// distinct directories; the same repository always gets the same one back.
pub fn repository_id(canonical_repo: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical_repo.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    digest.iter().fold(String::new(), |mut out, b| {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// `~/.oat/consoles/<repository id>/`, created on first use and surviving restarts (ticket
/// Contracts).
pub fn console_dir(env: &dyn Environment, canonical_repo: &Path) -> Result<PathBuf> {
    let home = env
        .home_dir()
        .ok_or_else(|| err(codes::INTERNAL_ERROR, "no home directory available"))?;
    Ok(home.join(".oat").join("consoles").join(repository_id(canonical_repo)))
}

fn record_path(dir: &Path) -> PathBuf {
    dir.join("console.json")
}

pub fn load_record(dir: &Path) -> Result<Option<ConsoleRecord>> {
    let path = record_path(dir);
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path)?;
    Ok(Some(serde_json::from_str(&content)?))
}

fn save_record(dir: &Path, record: &ConsoleRecord) -> Result<()> {
    fs::write(record_path(dir), serde_json::to_string_pretty(record)?)?;
    Ok(())
}

fn render_preamble(repo: &str, backend: Backend, console_dir: &str) -> String {
    prompt::PREAMBLE_CONSOLE
        .replace("{{ROLE_NAME}}", CoreRole::Console.name())
        .replace("{{REPOSITORY}}", repo)
        .replace("{{BACKEND_LABEL}}", backend.label())
        .replace("{{CONSOLE_DIR}}", console_dir)
}

fn render_baseline(repo: &str) -> String {
    prompt::OAT_CONSOLE_BASELINE.replace("{{REPOSITORY}}", repo)
}

/// Opens the console for `repo`: creates its directory on first use, materializes the
/// catalog's `oat-console` skills into it, assembles its prompt from the preamble, the
/// baseline (naming this repository and the prohibitions ADR-0004/ADR-0005 require), and the
/// catalog's instructions — in that order — and starts its session. Reopening a console whose
/// session is still alive is a no-op that returns the existing record.
pub fn open(
    env: &dyn Environment,
    catalog: &dyn RoleCatalog,
    repo: &Path,
    backend: Backend,
) -> Result<ConsoleRecord> {
    let repo = repo
        .canonicalize()
        .map_err(|e| err(codes::INVALID_INPUT, format!("invalid --repo: {e}")))?;
    let dir = console_dir(env, &repo)?;
    fs::create_dir_all(&dir)?;

    let tmux = Tmux::from_env(env);
    let session = Tmux::session_name(&format!("console-{}", repository_id(&repo)));

    if tmux.session_alive(&session).unwrap_or(false) {
        if let Some(record) = load_record(&dir)? {
            return Ok(record);
        }
    }

    let core_role = catalog.core_role(CoreRole::Console)?;
    let model = core_role.models.get(&backend).cloned().unwrap_or_default();

    skills::materialize_skills_plain(&dir, backend, &core_role.skills)?;

    let repo_label = repo.to_string_lossy().to_string();
    let preamble = render_preamble(&repo_label, backend, &dir.to_string_lossy());
    let baseline = render_baseline(&repo_label);

    let full_prompt = prompt::assemble(
        prompt::PromptParts {
            preamble,
            baseline: &baseline,
            instructions: std::slice::from_ref(&core_role.instructions),
            task: "",
        },
        backend,
    );

    fs::write(dir.join("prompt.md"), &full_prompt)?;
    let script = launch::launch_script(backend);
    let script_path = dir.join("launch.sh");
    fs::write(&script_path, &script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))?;
    }

    let mut env_vars = vec![
        ("OAT_WORKTREE".to_string(), dir.to_string_lossy().to_string()),
        ("OAT_DISPATCH_DIR".to_string(), dir.to_string_lossy().to_string()),
    ];
    if let Some(m) = &model.model {
        env_vars.push(("OAT_MODEL".to_string(), m.clone()));
    }
    if let Some(e) = &model.reasoning_effort {
        env_vars.push(("OAT_EFFORT".to_string(), e.clone()));
    }
    for name in launch::PASS_THROUGH_VARS {
        if let Some(v) = env.var(name) {
            env_vars.push((name.to_string(), v));
        }
    }

    tmux.start_session(&session, &dir, &env_vars, &script_path.to_string_lossy())?;

    let record = ConsoleRecord {
        repo: repo_label,
        backend: format!("{backend:?}").to_lowercase(),
        created_at: now_iso(),
        session,
    };
    save_record(&dir, &record)?;
    Ok(record)
}

/// Stops a repository's console session; its directory and skills are left in place so
/// reopening it does not repeat the first-use setup.
pub fn stop(env: &dyn Environment, repo: &Path) -> Result<ConsoleRecord> {
    let repo = repo
        .canonicalize()
        .map_err(|e| err(codes::INVALID_INPUT, format!("invalid --repo: {e}")))?;
    let dir = console_dir(env, &repo)?;
    let record = load_record(&dir)?
        .ok_or_else(|| err(codes::CONSOLE_NOT_FOUND, format!("no console open for '{}'", repo.display())))?;
    let tmux = Tmux::from_env(env);
    let _ = tmux.kill_session(&record.session);
    Ok(record)
}

/// The console's record and whether its session is currently alive, or `None` if this
/// repository has never opened one.
pub fn status(env: &dyn Environment, repo: &Path) -> Result<Option<(ConsoleRecord, bool)>> {
    let repo = repo
        .canonicalize()
        .map_err(|e| err(codes::INVALID_INPUT, format!("invalid --repo: {e}")))?;
    let dir = console_dir(env, &repo)?;
    let Some(record) = load_record(&dir)? else {
        return Ok(None);
    };
    let tmux = Tmux::from_env(env);
    let alive = tmux.session_alive(&record.session).unwrap_or(false);
    Ok(Some((record, alive)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_id_is_stable_and_distinct_per_path() {
        let a = PathBuf::from("/repos/one");
        let b = PathBuf::from("/repos/two");
        assert_eq!(repository_id(&a), repository_id(&a));
        assert_ne!(repository_id(&a), repository_id(&b));
    }

    #[test]
    fn console_baseline_names_the_repository_and_no_plugin_role() {
        let rendered = render_baseline("/repos/one");
        assert!(rendered.contains("/repos/one"));
        assert!(rendered.to_lowercase().contains("never consume a run inbox"));
        assert!(rendered.to_lowercase().contains("never release a dispatch"));
        assert!(rendered.to_lowercase().contains("never fire a role"));
        assert!(rendered.to_lowercase().contains("interactive panel"));
        for word in ["planner", "worker", "reviewer"] {
            assert!(!rendered.contains(word), "baseline names a plugin role: {word}");
        }
    }
}
