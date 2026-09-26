//! Profile scaffolding: the function `init` (F5) calls to give a fresh repository a working
//! execution profile, without ever overwriting a file or a profile that is already there
//! (ticket Contracts).

use super::config::{self, WorkspaceKind};
use crate::environment::Environment;
use anyhow::{Context, Result};
use std::fmt::Write as _;
use std::path::Path;

/// What this machine can tell the scaffolder about how it provides environments. `init`
/// gathers these; nothing here guesses them.
pub struct MachineFacts {
    pub context: String,
    pub namespace: String,
    pub workspace: WorkspaceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOutcome {
    Created,
    LeftAlone,
}

pub struct ScaffoldReport {
    pub repo_file: FileOutcome,
    pub machine_file: FileOutcome,
}

/// Writes `<repo>/.oat/exec.toml` (naming the profile) and `~/.oat/exec.toml` (describing how
/// this machine provides it), each only when that profile is not already declared there. A
/// file that exists but lacks this profile gets a new `[profile.*]` table appended; nothing
/// already in either file is rewritten.
pub fn scaffold(environment: &dyn Environment, repo: &Path, profile_name: &str, facts: &MachineFacts) -> Result<ScaffoldReport> {
    let repo_file = scaffold_repo_file(&repo.join(config::REPO_CONFIG), profile_name)?;
    let machine_file = match environment.home_dir() {
        Some(home) => scaffold_machine_file(&home.join(config::MACHINE_CONFIG), profile_name, facts)?,
        None => FileOutcome::LeftAlone,
    };
    Ok(ScaffoldReport { repo_file, machine_file })
}

/// The repository only ever names the profile it expects; it carries no machine-specific
/// facts, so a second machine never has to edit it to add itself.
fn scaffold_repo_file(path: &Path, profile_name: &str) -> Result<FileOutcome> {
    let existing = read(path)?;
    match existing {
        Some(config) if config.default_profile.as_deref() == Some(profile_name) => Ok(FileOutcome::LeftAlone),
        Some(_) => Ok(FileOutcome::LeftAlone),
        None => {
            write(path, &format!("default_profile = \"{profile_name}\"\n"))?;
            Ok(FileOutcome::Created)
        }
    }
}

fn scaffold_machine_file(path: &Path, profile_name: &str, facts: &MachineFacts) -> Result<FileOutcome> {
    let existing = read(path)?;
    if let Some(config) = &existing {
        if config.profile.contains_key(profile_name) {
            return Ok(FileOutcome::LeftAlone);
        }
    }
    let mut addition = String::new();
    let _ = writeln!(addition, "[profile.{profile_name}]");
    let _ = writeln!(addition, "context = \"{}\"", facts.context);
    let _ = writeln!(addition, "namespace = \"{}\"", facts.namespace);
    if facts.workspace != WorkspaceKind::default() {
        let _ = writeln!(addition, "workspace = \"{}\"", workspace_str(facts.workspace));
    }
    match existing {
        Some(_) => append(path, &addition)?,
        None => write(path, &addition)?,
    }
    Ok(FileOutcome::Created)
}

fn workspace_str(kind: WorkspaceKind) -> &'static str {
    match kind {
        WorkspaceKind::Hostpath => "hostpath",
    }
}

fn read(path: &Path) -> Result<Option<config::ExecConfig>> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(None);
    };
    let config: config::ExecConfig = toml::from_str(&text).with_context(|| format!("{} is not a valid exec config", path.display()))?;
    Ok(Some(config))
}

fn write(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(path, body).with_context(|| format!("failed to write {}", path.display()))
}

fn append(path: &Path, body: &str) -> Result<()> {
    use std::io::Write;
    let mut existing = std::fs::read_to_string(path).unwrap_or_default();
    if !existing.is_empty() && !existing.ends_with('\n') {
        existing.push('\n');
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    file.write_all(existing.as_bytes())?;
    file.write_all(b"\n")?;
    file.write_all(body.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::TestEnvironment;

    fn facts() -> MachineFacts {
        MachineFacts {
            context: "dev".to_owned(),
            namespace: "agents".to_owned(),
            workspace: WorkspaceKind::Hostpath,
        }
    }

    #[test]
    fn a_fresh_repository_gets_both_files() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let home = directory.path().join("home");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let environment = TestEnvironment::new(home.clone());

        let report = scaffold(&environment, &repo, "local", &facts()).unwrap();
        assert_eq!(report.repo_file, FileOutcome::Created);
        assert_eq!(report.machine_file, FileOutcome::Created);

        let config = config::load(&environment, Some(&repo)).unwrap();
        let (name, profile) = config::resolve(&config, None).unwrap();
        assert_eq!(name, "local");
        assert_eq!(profile.context, "dev");
        assert_eq!(profile.namespace, "agents");
    }

    #[test]
    fn an_existing_profile_of_the_same_name_is_left_alone() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let home = directory.path().join("home");
        std::fs::create_dir_all(repo.join(".oat")).unwrap();
        std::fs::create_dir_all(home.join(".oat")).unwrap();
        std::fs::write(repo.join(config::REPO_CONFIG), "default_profile = \"local\"\n").unwrap();
        std::fs::write(
            home.join(config::MACHINE_CONFIG),
            "[profile.local]\ncontext = \"already-here\"\nnamespace = \"agents\"\n",
        )
        .unwrap();
        let environment = TestEnvironment::new(home.clone());

        let report = scaffold(&environment, &repo, "local", &facts()).unwrap();
        assert_eq!(report.repo_file, FileOutcome::LeftAlone);
        assert_eq!(report.machine_file, FileOutcome::LeftAlone);

        let config = config::load(&environment, Some(&repo)).unwrap();
        let (_, profile) = config::resolve(&config, None).unwrap();
        assert_eq!(profile.context, "already-here", "the existing profile was not rewritten");
    }

    #[test]
    fn a_new_profile_is_added_to_an_existing_machine_file_without_disturbing_the_rest() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let home = directory.path().join("home");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(home.join(".oat")).unwrap();
        std::fs::write(
            home.join(config::MACHINE_CONFIG),
            "[profile.other]\ncontext = \"c\"\nnamespace = \"n\"\n",
        )
        .unwrap();
        let environment = TestEnvironment::new(home.clone());

        let report = scaffold(&environment, &repo, "local", &facts()).unwrap();
        assert_eq!(report.machine_file, FileOutcome::Created);

        let config = config::load(&environment, Some(&repo)).unwrap();
        assert!(config.profile.contains_key("other"), "the pre-existing profile survived");
        assert!(config.profile.contains_key("local"), "the new profile was added");
    }
}
