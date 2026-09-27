// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

use super::{
    missing_core_instructions_error, unknown_role_error, CoreRole, CoreRoleDefinition,
    RoleCatalog, RoleDefinition,
};
use anyhow::Result;
use std::collections::BTreeMap;

/// An in-memory `RoleCatalog` built by `InMemoryCatalogBuilder`. This exists for tests: it is
/// this ticket's only implementation of the seam, so F4's plugin-backed catalog has a fixture
/// to test against without depending on the plugin loader it has not built yet.
pub struct InMemoryCatalog {
    roles: BTreeMap<String, RoleDefinition>,
    core_roles: BTreeMap<CoreRole, CoreRoleDefinition>,
}

impl RoleCatalog for InMemoryCatalog {
    fn role_names(&self) -> Vec<String> {
        self.roles.keys().cloned().collect()
    }

    fn role(&self, name: &str) -> Result<&RoleDefinition> {
        self.roles
            .get(name)
            .ok_or_else(|| unknown_role_error(self, name))
    }

    fn core_role(&self, role: CoreRole) -> Result<&CoreRoleDefinition> {
        self.core_roles
            .get(&role)
            .ok_or_else(|| missing_core_instructions_error(role))
    }
}

#[derive(Default)]
pub struct InMemoryCatalogBuilder {
    roles: BTreeMap<String, RoleDefinition>,
    core_roles: BTreeMap<CoreRole, CoreRoleDefinition>,
}

impl InMemoryCatalogBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_role(mut self, role: RoleDefinition) -> Self {
        self.roles.insert(role.name.clone(), role);
        self
    }

    pub fn with_core_role(mut self, role: CoreRole, definition: CoreRoleDefinition) -> Self {
        self.core_roles.insert(role, definition);
        self
    }

    pub fn build(self) -> InMemoryCatalog {
        InMemoryCatalog {
            roles: self.roles,
            core_roles: self.core_roles,
        }
    }
}
