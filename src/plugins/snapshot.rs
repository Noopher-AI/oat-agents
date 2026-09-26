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
/// role/core-role joining sees the same order the pins declared.
pub fn snapshot_plugins(resolved: &[ResolvedPlugin], dest_root: &Path) -> Result<Vec<PluginRecord>> {
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
}
