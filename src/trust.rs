//! The trust store (ticket Architecture): `~/.oat/trust.toml`, one entry per (source, version),
//! machine-only. Nothing under a repository is ever read as trust, and a changed pin voids
//! trust simply because the version half of the key changes with it.

use crate::environment::Environment;
use crate::error::{codes, err};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// A plugin's trust identity: what a grant is recorded against, and what a resolved plugin is
/// checked against before it is allowed to run. Two pins that differ in `commit` or
/// `content_hash` are two different keys, so re-pinning always requires a fresh grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum TrustKey {
    Embedded { name: String, version: String },
    Git { url: String, commit: String },
    Path { content_hash: String },
}

impl TrustKey {
    /// The `.oat/plugins.toml` `source` value this key corresponds to, for error messages and
    /// `run.json`/log records.
    pub fn source_label(&self) -> String {
        match self {
            TrustKey::Embedded { .. } => "embedded".to_string(),
            TrustKey::Git { url, .. } => format!("git:{url}"),
            TrustKey::Path { .. } => "path".to_string(),
        }
    }

    pub fn version(&self) -> &str {
        match self {
            TrustKey::Embedded { version, .. } => version,
            TrustKey::Git { commit, .. } => commit,
            TrustKey::Path { content_hash, .. } => content_hash,
        }
    }

    /// The exact `oat-agents plugin trust` invocation that grants this key, printed verbatim by
    /// the gate error so the operator never has to construct it by hand.
    pub fn trust_command(&self) -> String {
        match self {
            TrustKey::Embedded { name, version } => {
                format!("oat-agents plugin trust embedded --name {name} --version {version}")
            }
            TrustKey::Git { url, commit } => {
                format!("oat-agents plugin trust git --url {url} --commit {commit}")
            }
            TrustKey::Path { content_hash } => {
                format!("oat-agents plugin trust path --content-hash {content_hash}")
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustEntry {
    #[serde(flatten)]
    pub key: TrustKey,
    pub granted_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TrustFile {
    #[serde(default, rename = "trust")]
    entries: Vec<TrustEntry>,
}

pub struct TrustStore {
    path: PathBuf,
    file: TrustFile,
}

impl TrustStore {
    /// `~/.oat/trust.toml` — never a path inside a repository.
    pub fn path_for(env: &dyn Environment) -> Result<PathBuf> {
        let home = env
            .home_dir()
            .ok_or_else(|| err(codes::INTERNAL_ERROR, "no home directory available"))?;
        Ok(home.join(".oat").join("trust.toml"))
    }

    pub fn open(env: &dyn Environment) -> Result<Self> {
        Self::open_at(Self::path_for(env)?)
    }

    pub fn open_at(path: PathBuf) -> Result<Self> {
        let file = if path.exists() {
            let text = fs::read_to_string(&path)?;
            toml::from_str(&text).map_err(|e| err(codes::INTERNAL_ERROR, format!("{} is not a valid trust store: {e}", path.display())))?
        } else {
            TrustFile::default()
        };
        Ok(Self { path, file })
    }

    pub fn is_trusted(&self, key: &TrustKey) -> bool {
        self.file.entries.iter().any(|e| &e.key == key)
    }

    pub fn grant(&mut self, key: TrustKey, granted_at: String) -> Result<()> {
        if !self.is_trusted(&key) {
            self.file.entries.push(TrustEntry { key, granted_at });
        }
        self.save()
    }

    pub fn entries(&self) -> &[TrustEntry] {
        &self.file.entries
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.path, toml::to_string_pretty(&self.file)?)?;
        Ok(())
    }
}

/// The plugin/source/version an untrusted gate names, and the exact command that trusts it —
/// shared by the CLI's gate error and `plugin list`'s status column.
pub fn untrusted_plugin_error(declared_name: &str, key: &TrustKey) -> anyhow::Error {
    err(
        codes::PLUGIN_UNTRUSTED,
        format!(
            "plugin '{declared_name}' (source: {}, version: {}) is not trusted on this machine; run: {}",
            key.source_label(),
            key.version(),
            key.trust_command()
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::TestEnvironment;

    #[test]
    fn a_grant_is_recorded_and_a_different_version_is_still_untrusted() {
        let dir = tempfile::tempdir().unwrap();
        let env = TestEnvironment::new(dir.path().to_path_buf());
        let mut store = TrustStore::open(&env).unwrap();
        let key = TrustKey::Git {
            url: "https://example.com/plugin.git".to_string(),
            commit: "a".repeat(40),
        };
        assert!(!store.is_trusted(&key));
        store.grant(key.clone(), "2026-01-01T00:00:00Z".to_string()).unwrap();
        assert!(store.is_trusted(&key));

        let reopened = TrustStore::open(&env).unwrap();
        assert!(reopened.is_trusted(&key));

        let changed = TrustKey::Git {
            url: "https://example.com/plugin.git".to_string(),
            commit: "b".repeat(40),
        };
        assert!(!reopened.is_trusted(&changed), "changing the pin voids trust");
    }

    #[test]
    fn granting_the_same_key_twice_does_not_duplicate_it() {
        let dir = tempfile::tempdir().unwrap();
        let env = TestEnvironment::new(dir.path().to_path_buf());
        let mut store = TrustStore::open(&env).unwrap();
        let key = TrustKey::Path { content_hash: "sha256:aa".to_string() };
        store.grant(key.clone(), "t1".to_string()).unwrap();
        store.grant(key.clone(), "t2".to_string()).unwrap();
        assert_eq!(store.entries().len(), 1);
    }
}
