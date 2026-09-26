//! The snapshot (ticket Architecture): once plugins are resolved and trusted, their trees are
//! copied into the Run's (or console's) own state directory, and every later catalog is built
//! from that copy rather than by re-resolving pins.

use super::ResolvedPlugin;
use crate::error::{codes, err};
use crate::store::PluginRecord;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestEntry {
    name: String,
    source: String,
    version: String,
    dir_name: String,
}

/// Copies every resolved plugin's tree into `dest_root`, one subdirectory per plugin in pin
/// order, and writes a manifest recording that order — `catalog_from_snapshot` reads it back so
/// role/core-role joining sees the same order the pins declared. `dest_root` is replaced, not
/// merged into: reopening a console (or any other caller) that already has a snapshot at this
/// path gets a snapshot that exactly matches the current pins, so content removed upstream since
/// the last snapshot does not linger and get loaded again.
pub fn snapshot_plugins(resolved: &[ResolvedPlugin], dest_root: &Path) -> Result<Vec<PluginRecord>> {
    if dest_root.exists() {
        fs::remove_dir_all(dest_root)?;
    }
    fs::create_dir_all(dest_root)?;
    let mut manifest = Vec::new();
    let mut records = Vec::new();
    for (index, plugin) in resolved.iter().enumerate() {
        let dir_name = format!("{index:03}-{}", crate::worktree::sanitize_segment(&plugin.declared_name));
        copy_dir_recursive(&plugin.dir, &dest_root.join(&dir_name))?;
        manifest.push(ManifestEntry {
            name: plugin.declared_name.clone(),
            source: plugin.source_label(),
            version: plugin.version(),
            dir_name,
        });
        records.push(PluginRecord {
            name: plugin.declared_name.clone(),
            source: plugin.source_label(),
            version: plugin.version(),
        });
    }
    fs::write(dest_root.join("manifest.json"), serde_json::to_string_pretty(&manifest)?)?;
    Ok(records)
}

fn manifest_path(dest_root: &Path) -> PathBuf {
    dest_root.join("manifest.json")
}

fn read_manifest(dest_root: &Path) -> Result<Vec<ManifestEntry>> {
    let path = manifest_path(dest_root);
    let text = fs::read_to_string(&path).map_err(|e| {
        err(
            codes::INTERNAL_ERROR,
            format!("no plugin snapshot at '{}': {e}", dest_root.display()),
        )
    })?;
    Ok(serde_json::from_str(&text)?)
}

/// The snapshot's plugin directories, in pin order, ready to pass to `plugin::load_catalog`.
pub fn snapshot_dirs(dest_root: &Path) -> Result<Vec<PathBuf>> {
    Ok(read_manifest(dest_root)?.into_iter().map(|e| dest_root.join(e.dir_name)).collect())
}

/// Loads the `RoleCatalog` a Run's (or console's) snapshot was recorded with, without
/// re-resolving or re-trust-checking any pin — that already happened once, at `gate` time.
pub fn catalog_from_snapshot(dest_root: &Path) -> Result<crate::plugin::PluginCatalog> {
    let dirs = snapshot_dirs(dest_root)?;
    let (catalog, _refs) = crate::plugin::load_catalog(&dirs).map_err(|errors| crate::plugin::errors_to_cli_failure(&errors))?;
    Ok(catalog)
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dest.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &target)?;
        } else if path.is_file() {
            fs::copy(&path, &target)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = fs::metadata(&path)?.permissions().mode();
                fs::set_permissions(&target, fs::Permissions::from_mode(mode))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trust::TrustKey;

    #[test]
    fn a_snapshot_round_trips_into_a_loadable_catalog_in_pin_order() {
        let temp = tempfile::tempdir().unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample-plugin");
        let resolved = vec![ResolvedPlugin {
            declared_name: "sample-plugin".to_string(),
            dir: fixtures,
            trust_key: TrustKey::Path { content_hash: "sha256:aa".to_string() },
        }];

        let dest = temp.path().join("snapshot");
        let records = snapshot_plugins(&resolved, &dest).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "sample-plugin");

        let catalog = catalog_from_snapshot(&dest).unwrap();
        use crate::role::RoleCatalog;
        assert!(catalog.role_names().contains(&"sample-role-fresh".to_string()));
    }

    fn minimal_plugin_source(dir: &Path, name: &str, role_name: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join("oat-plugin.toml"),
            format!("format_version = 1\nname = \"{name}\"\ndescription = \"d\"\n"),
        )
        .unwrap();
        let role_dir = dir.join("roles").join(role_name);
        fs::create_dir_all(&role_dir).unwrap();
        fs::write(
            role_dir.join("role.toml"),
            "description = \"d\"\nstart = \"fresh\"\nexec_environment = false\nprior_verification = false\n",
        )
        .unwrap();
        fs::write(role_dir.join("instructions.md"), format!("# {role_name}\n")).unwrap();

        let core_dir = dir.join("core");
        fs::create_dir_all(&core_dir).unwrap();
        fs::write(core_dir.join("oat-meta-instruction.md"), "# oat-meta\n").unwrap();
        fs::write(core_dir.join("oat-console-instruction.md"), "# oat-console\n").unwrap();
    }

    #[test]
    fn reopening_a_console_replaces_the_snapshot_instead_of_merging_into_it() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        minimal_plugin_source(&source, "reopen-plugin", "role-a");

        let resolved = vec![ResolvedPlugin {
            declared_name: "reopen-plugin".to_string(),
            dir: source.clone(),
            trust_key: TrustKey::Path { content_hash: "sha256:aa".to_string() },
        }];

        let dest = temp.path().join("console").join("plugins");
        snapshot_plugins(&resolved, &dest).unwrap();

        use crate::role::RoleCatalog;
        let first_catalog = catalog_from_snapshot(&dest).unwrap();
        assert!(first_catalog.role_names().contains(&"role-a".to_string()));

        // The upstream source drops role-a and gains role-b, then the console is reopened:
        // the same console directory is snapshotted into again.
        fs::remove_dir_all(source.join("roles").join("role-a")).unwrap();
        let role_b_dir = source.join("roles").join("role-b");
        fs::create_dir_all(&role_b_dir).unwrap();
        fs::write(
            role_b_dir.join("role.toml"),
            "description = \"d\"\nstart = \"fresh\"\nexec_environment = false\nprior_verification = false\n",
        )
        .unwrap();
        fs::write(role_b_dir.join("instructions.md"), "# role-b\n").unwrap();

        snapshot_plugins(&resolved, &dest).unwrap();
        let second_catalog = catalog_from_snapshot(&dest).unwrap();
        let names = second_catalog.role_names();
        assert!(names.contains(&"role-b".to_string()), "expected role-b in {names:?}");
        assert!(!names.contains(&"role-a".to_string()), "role-a should no longer be loadable, got {names:?}");
    }
}
