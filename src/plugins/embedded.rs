//! The embedded source (ticket Architecture): the example plugin's tree, embedded at build
//! time by `build.rs` from `plugins/example/` in this repository. Its content hash is checked
//! by a test to equal the hash of that directory, so the embedded copy and the directory can
//! never drift apart. `plugins/example/` is F6's to write; this module only knows how to embed
//! and materialize whatever is there — it never writes or assumes any of its content.

use crate::error::{codes, err};
use crate::hashing::{hash_entries, HashedFile};
use anyhow::Result;
use std::fs;
use std::path::Path;

pub struct EmbeddedFile {
    pub relative_path: &'static str,
    pub mode: u32,
    pub bytes: &'static [u8],
}

/// The only embedded plugin this build understands. A `.oat/plugins.toml` naming any other
/// embedded plugin is refused (ticket Scope: "the embedded-plugin mechanism").
pub const EXAMPLE_NAME: &str = "example";

const EXAMPLE_FILES: &[EmbeddedFile] = include!(concat!(env!("OUT_DIR"), "/embedded_example.rs"));

/// The embedded example's version: always the binary's own version (ticket Architecture:
/// "`example` pins to the binary's version").
pub fn example_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The embedded example's content hash, using the exact same encoding `hashing::hash_dir` uses
/// for an in-repository plugin, so the two are directly comparable.
pub fn example_content_hash() -> String {
    hash_entries(EXAMPLE_FILES.iter().map(|f| HashedFile {
        relative_path: f.relative_path.to_string(),
        mode: f.mode,
        bytes: f.bytes,
    }))
}

/// Writes the embedded example's files under `dest`, creating directories as needed and
/// preserving each file's embedded mode on Unix. A no-op (empty directory) when
/// `plugins/example/` did not exist at build time.
pub fn materialize_example(dest: &Path) -> Result<()> {
    for file in EXAMPLE_FILES {
        let path = dest.join(file.relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, file.bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(file.mode))?;
        }
    }
    Ok(())
}

/// Whether the embedded example currently has any content — `false` once `plugins/example/`
/// exists in the repository at build time; `true` only in a build from before it did. Its
/// value is a compile-time constant either way, which is exactly what clippy's `const_is_empty`
/// flags; the `allow` covers both states of that constant, not just the current one.
#[allow(clippy::const_is_empty)]
pub fn is_empty() -> bool {
    EXAMPLE_FILES.is_empty()
}

pub fn require_known_embedded_plugin(name: &str) -> Result<()> {
    if name != EXAMPLE_NAME {
        return Err(err(
            codes::PLUGIN_LOAD_FAILED,
            format!("unknown embedded plugin '{name}'; this build only embeds '{EXAMPLE_NAME}'"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ticket's own self-check: the embedded copy and `plugins/example/` in the repository
    /// can never drift apart. While `plugins/example/` does not exist yet (F6), both sides hash
    /// an empty file list and trivially agree; once F6 adds it, this starts checking the real
    /// thing without any change here.
    #[test]
    fn the_embedded_example_matches_the_repository_directory() {
        let repo_example = Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins").join("example");
        let repo_hash = if repo_example.is_dir() {
            crate::hashing::hash_dir(&repo_example).unwrap()
        } else {
            hash_entries(std::iter::empty::<HashedFile>())
        };
        let embedded_hash = hash_entries(EXAMPLE_FILES.iter().map(|f| HashedFile {
            relative_path: f.relative_path.to_string(),
            mode: f.mode,
            bytes: f.bytes,
        }));
        assert_eq!(embedded_hash, repo_hash);
    }

    #[test]
    fn an_unknown_embedded_plugin_name_is_refused() {
        assert!(require_known_embedded_plugin("example").is_ok());
        assert!(require_known_embedded_plugin("something-else").is_err());
    }
}
