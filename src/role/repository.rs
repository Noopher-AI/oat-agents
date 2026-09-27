// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

//! `.oat/roles.toml`: what a repository sets for the roles its Runs use, over the defaults its
//! plugins declare — a role's concurrency limit (ADR-0006), and the backend and per-backend
//! model a role is launched with (ADR-0007).
//!
//! ```toml
//! [worker]
//! max_concurrent = 2
//! backend = "codex"
//!
//! [worker.model.codex]
//! model = "gpt-5.5"
//! reasoning_effort = "high"
//!
//! [oat-meta.model.claude]
//! model = "opus"
//! ```
//!
//! A table may name any plugin role of the Run, or `oat-meta` or `oat-console`; a core role
//! takes no `max_concurrent`.

use crate::error::{codes, err};
use crate::plugin::format::ModelToml;
use crate::role::{Backend, CoreRole, ModelSetting, RoleCatalog};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::str::FromStr;

pub const REPO_CONFIG: &str = ".oat/roles.toml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleEntry {
    #[serde(default)]
    max_concurrent: Option<u32>,
    #[serde(default)]
    backend: Option<String>,
    #[serde(default)]
    model: BTreeMap<String, ModelToml>,
}

/// What the repository sets for how one role is launched. Anything it leaves out keeps what
/// the plugin declares, or the Run's backend.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<Backend>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub model: BTreeMap<Backend, ModelSetting>,
}

impl LaunchSettings {
    /// The model a launch on `backend` gets: the plugin's, with each field the repository sets
    /// for that backend replacing the plugin's.
    pub fn model_for(&self, backend: Backend, plugin: &BTreeMap<Backend, ModelSetting>) -> ModelSetting {
        let mut model = plugin.get(&backend).cloned().unwrap_or_default();
        if let Some(repo) = self.model.get(&backend) {
            if repo.model.is_some() {
                model.model = repo.model.clone();
            }
            if repo.reasoning_effort.is_some() {
                model.reasoning_effort = repo.reasoning_effort.clone();
            }
        }
        model
    }
}

/// Everything `.oat/roles.toml` sets, by role name, checked against the Run's roles.
#[derive(Debug, Default)]
pub struct RepoRoles {
    pub limits: BTreeMap<String, u32>,
    pub launch: BTreeMap<String, LaunchSettings>,
}

impl RepoRoles {
    /// The launch settings for `role`, empty when the repository names it nowhere.
    pub fn launch_for(&self, role: &str) -> LaunchSettings {
        self.launch.get(role).cloned().unwrap_or_default()
    }
}

/// Reads `<repo>/.oat/roles.toml`; a repository without one sets nothing.
pub fn load(repo: &Path, catalog: &dyn RoleCatalog) -> Result<RepoRoles> {
    let contents = match fs::read_to_string(repo.join(REPO_CONFIG)) {
        Ok(contents) => contents,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(RepoRoles::default()),
        Err(e) => return Err(err(codes::ROLE_LIMITS_INVALID, format!("could not read {REPO_CONFIG}: {e}"))),
    };
    parse(&contents, catalog)
}

fn parse(contents: &str, catalog: &dyn RoleCatalog) -> Result<RepoRoles> {
    let entries: BTreeMap<String, RoleEntry> = toml::from_str(contents)
        .map_err(|e| err(codes::ROLE_LIMITS_INVALID, format!("{REPO_CONFIG}: {e}")))?;
    let core_roles = [CoreRole::Meta.name(), CoreRole::Console.name()];
    let mut out = RepoRoles::default();
    for (role, entry) in entries {
        let is_core = core_roles.contains(&role.as_str());
        if !is_core && catalog.role(&role).is_err() {
            let mut known = catalog.role_names();
            known.extend(core_roles.iter().map(|name| name.to_string()));
            known.sort();
            return Err(err(
                codes::ROLE_LIMITS_INVALID,
                format!(
                    "{REPO_CONFIG} names role '{role}', which no plugin of this Run supplies; known roles: {}",
                    known.join(", ")
                ),
            ));
        }

        if let Some(limit) = entry.max_concurrent {
            if is_core {
                return Err(err(
                    codes::ROLE_LIMITS_INVALID,
                    format!("{REPO_CONFIG}: '{role}' is a core role and takes no max_concurrent"),
                ));
            }
            if limit == 0 {
                return Err(err(
                    codes::ROLE_LIMITS_INVALID,
                    format!("{REPO_CONFIG}: max_concurrent for '{role}' must be at least 1"),
                ));
            }
            out.limits.insert(role.clone(), limit);
        }

        let mut settings = LaunchSettings::default();
        if let Some(name) = &entry.backend {
            settings.backend = Some(backend(name, &format!("backend for '{role}'"))?);
        }
        for (key, value) in entry.model {
            let backend = backend(&key, &format!("[{role}.model.{key}]"))?;
            settings.model.insert(
                backend,
                ModelSetting { model: value.model, reasoning_effort: value.reasoning_effort },
            );
        }
        if settings != LaunchSettings::default() {
            out.launch.insert(role, settings);
        }
    }
    Ok(out)
}

fn backend(name: &str, context: &str) -> Result<Backend> {
    Backend::from_str(name).map_err(|_| {
        err(
            codes::ROLE_SETTINGS_INVALID,
            format!("{REPO_CONFIG}: unknown backend '{name}' in {context}; expected claude or codex"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::to_failure;
    use crate::role::memory::InMemoryCatalogBuilder;
    use crate::role::{RoleDefinition, StartLocation};

    fn catalog() -> crate::role::memory::InMemoryCatalog {
        InMemoryCatalogBuilder::new()
            .with_role(RoleDefinition {
                name: "worker".to_string(),
                instructions: "Throw the pots.".to_string(),
                models: BTreeMap::new(),
                skills: Vec::new(),
                mcp_servers: Vec::new(),
                start: StartLocation::Fresh,
                exec_environment: false,
                prior_verification: false,
                max_concurrent: None,
            })
            .build()
    }

    fn setting(model: Option<&str>, effort: Option<&str>) -> ModelSetting {
        ModelSetting { model: model.map(str::to_owned), reasoning_effort: effort.map(str::to_owned) }
    }

    #[test]
    fn a_role_and_both_core_roles_take_a_backend_and_a_model() {
        let parsed = parse(
            "[worker]\nmax_concurrent = 2\nbackend = \"codex\"\n\
             [worker.model.codex]\nmodel = \"gpt-5.5\"\nreasoning_effort = \"high\"\n\
             [oat-meta.model.claude]\nmodel = \"opus\"\n\
             [oat-console]\nbackend = \"codex\"\n",
            &catalog(),
        )
        .unwrap();
        assert_eq!(parsed.limits.get("worker"), Some(&2));
        let worker = parsed.launch_for("worker");
        assert_eq!(worker.backend, Some(Backend::Codex));
        assert_eq!(worker.model[&Backend::Codex], setting(Some("gpt-5.5"), Some("high")));
        assert_eq!(parsed.launch_for("oat-meta").model[&Backend::Claude], setting(Some("opus"), None));
        assert_eq!(parsed.launch_for("oat-console").backend, Some(Backend::Codex));
    }

    #[test]
    fn a_field_the_repository_leaves_out_keeps_the_plugins() {
        let parsed = parse("[worker.model.claude]\nreasoning_effort = \"low\"\n", &catalog()).unwrap();
        let plugin = BTreeMap::from([(Backend::Claude, setting(Some("sonnet"), Some("high")))]);
        let worker = parsed.launch_for("worker");
        assert_eq!(worker.model_for(Backend::Claude, &plugin), setting(Some("sonnet"), Some("low")));
        assert_eq!(worker.model_for(Backend::Codex, &plugin), ModelSetting::default());
    }

    #[test]
    fn an_unknown_backend_is_refused() {
        for contents in ["[worker]\nbackend = \"gemini\"\n", "[worker.model.gemini]\nmodel = \"x\"\n"] {
            let error = parse(contents, &catalog()).unwrap_err();
            assert_eq!(to_failure(&error).code, codes::ROLE_SETTINGS_INVALID, "{contents}");
        }
    }

    #[test]
    fn a_core_role_takes_no_limit() {
        let error = parse("[oat-meta]\nmax_concurrent = 1\n", &catalog()).unwrap_err();
        assert_eq!(to_failure(&error).code, codes::ROLE_LIMITS_INVALID);
    }
}
