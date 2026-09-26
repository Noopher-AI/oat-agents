pub mod catalog;
pub mod format;
pub mod load;

use crate::error::{codes, CliFailure};
use std::path::PathBuf;

pub use catalog::PluginCatalog;
pub use load::LoadedPlugin;

/// One error found while loading or combining plugins. Every kind of failure this module
/// produces names the plugin directory and, where the problem is file-shaped, the file inside
/// it — never a bare message (ticket: "every error names the plugin, the file and the
/// problem").
#[derive(Debug, Clone)]
pub struct PluginError {
    pub plugin: Option<PathBuf>,
    pub file: Option<String>,
    pub message: String,
}

impl PluginError {
    pub fn new(plugin: &std::path::Path, file: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            plugin: Some(plugin.to_path_buf()),
            file: Some(file.into()),
            message: message.into(),
        }
    }

    /// An error about the combination of several plugins (a role or core-role conflict); the
    /// message names every plugin involved, so no single `plugin` field would be enough.
    pub fn combination(message: impl Into<String>) -> Self {
        Self {
            plugin: None,
            file: None,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.plugin, &self.file) {
            (Some(plugin), Some(file)) => {
                write!(f, "plugin '{}', file '{file}': {}", plugin.display(), self.message)
            }
            (Some(plugin), None) => write!(f, "plugin '{}': {}", plugin.display(), self.message),
            (None, _) => write!(f, "{}", self.message),
        }
    }
}

impl std::error::Error for PluginError {}

/// Turns a batch of plugin errors into the crate's stable CLI failure shape, so a caller that
/// only knows how to report a `CliFailure` (F5's `.oat/plugins.toml` loading, `oat-agents env
/// doctor`-style commands) does not have to know this module's error type.
pub fn errors_to_cli_failure(errors: &[PluginError]) -> anyhow::Error {
    let details = serde_json::Value::Array(
        errors
            .iter()
            .map(|e| {
                serde_json::json!({
                    "plugin": e.plugin.as_ref().map(|p| p.display().to_string()),
                    "file": e.file,
                    "message": e.message,
                })
            })
            .collect(),
    );
    let summary = errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    anyhow::Error::new(CliFailure::new(codes::PLUGIN_LOAD_FAILED, summary).with_details(details))
}

/// The Loader (ticket Contracts): given an ordered list of plugin directories, returns the
/// combined `RoleCatalog` or every error found across all of them, plus each plugin's declared
/// name so a caller (F5) can match it against a pin. Loading every plugin before combining
/// means a directory that fails to load and a role-name conflict elsewhere are both reported
/// in one run.
#[derive(Debug)]
pub struct PluginRef {
    pub name: String,
    pub dir: PathBuf,
}

pub fn load_catalog(dirs: &[PathBuf]) -> Result<(PluginCatalog, Vec<PluginRef>), Vec<PluginError>> {
    let mut errors = Vec::new();
    let mut plugins = Vec::new();
    for dir in dirs {
        match load::load_plugin(dir) {
            Ok(plugin) => plugins.push(plugin),
            Err(mut e) => errors.append(&mut e),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let refs = plugins
        .iter()
        .map(|p| PluginRef {
            name: p.name.clone(),
            dir: p.dir.clone(),
        })
        .collect();

    let catalog = catalog::build_catalog(&plugins)?;
    Ok((catalog, refs))
}
