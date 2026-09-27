// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

//! The image a role runs in, pinned to something that can be compared.
//!
//! A tag is a moving target; an image id is not. Two dispatches that report the same image id
//! ran under conditions anyone can check, rather than assume.

use super::config::{ImageBuilder, ImageLoad, Profile};
use crate::error::{codes, err};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct ImageRef {
    pub reference: String,
    pub id: String,
    pub source_hash: String,
    pub rebuilt: bool,
}

/// The container definition the repository owns. Everything under it feeds the hash, so any
/// change to the environment produces a different image.
pub const DEVCONTAINER_DIR: &str = ".devcontainer";

pub fn devcontainer_dir(worktree: &Path) -> PathBuf {
    worktree.join(DEVCONTAINER_DIR)
}

/// A content hash of the whole `.devcontainer/` directory: paths and bytes, in a stable order.
pub fn source_hash(worktree: &Path) -> Result<String> {
    let root = devcontainer_dir(worktree);
    if !root.is_dir() {
        return Err(err(
            codes::ENV_IMAGE_BUILD_FAILED,
            format!(
                "{} has no {DEVCONTAINER_DIR}/ directory, so there is no image to build",
                worktree.display()
            ),
        ));
    }
    let mut files = Vec::new();
    collect(&root, &root, &mut files)?;
    files.sort();
    let mut hasher = Sha256::new();
    for relative in files {
        let absolute = root.join(&relative);
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update([0]);
        hasher.update(
            std::fs::read(&absolute).with_context(|| format!("failed to read {}", absolute.display()))?,
        );
        hasher.update([0]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect(root: &Path, dir: &Path, into: &mut Vec<PathBuf>) -> Result<()> {
    let entries = std::fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("failed to read {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, into)?;
        } else if let Ok(relative) = path.strip_prefix(root) {
            into.push(relative.to_path_buf());
        }
    }
    Ok(())
}

/// Builds the image only when `.devcontainer/` has changed since the tag that names its hash
/// was built.
pub fn ensure(profile: &Profile, worktree: &Path, force: bool) -> Result<ImageRef> {
    let hash = source_hash(worktree)?;
    let short = &hash[..12];
    let name = super::sanitize(
        worktree
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "workspace".to_owned())
            .as_str(),
    );
    let reference = match (&profile.image.registry, profile.image.load) {
        (Some(registry), ImageLoad::Push) => format!("{registry}/oat-{name}:{short}"),
        _ => format!("oat-{name}:{short}"),
    };
    // What the cluster pulls by and what this machine can push to are not always the same
    // address, and the pod only ever sees the first.
    let push_reference = match (&profile.image.push_to, profile.image.load) {
        (Some(endpoint), ImageLoad::Push) => format!("{endpoint}/oat-{name}:{short}"),
        _ => reference.clone(),
    };

    if !force {
        if let Some(id) = inspect_id(&reference)? {
            return Ok(ImageRef {
                reference,
                id,
                source_hash: hash,
                rebuilt: false,
            });
        }
    }

    match profile.image.builder {
        ImageBuilder::Devcontainer => tool(
            "devcontainer",
            &[
                "build",
                "--workspace-folder",
                &worktree.to_string_lossy(),
                "--image-name",
                &reference,
            ],
        )?,
        ImageBuilder::Dockerfile => tool(
            "docker",
            &[
                "build",
                "-f",
                &devcontainer_dir(worktree).join("Dockerfile").to_string_lossy(),
                "-t",
                &reference,
                &worktree.to_string_lossy(),
            ],
        )?,
    };

    let id = inspect_id(&reference)?.ok_or_else(|| {
        err(
            codes::ENV_IMAGE_BUILD_FAILED,
            format!("{reference} was built but the local image store does not have it"),
        )
    })?;
    load(profile, &reference, &push_reference)?;

    Ok(ImageRef {
        reference,
        id,
        source_hash: hash,
        rebuilt: true,
    })
}

/// How a freshly built image reaches the nodes. Every cluster answers this differently, which
/// is exactly why it is configured and not inferred.
fn load(profile: &Profile, reference: &str, push_reference: &str) -> Result<()> {
    match profile.image.load {
        ImageLoad::None => Ok(()),
        ImageLoad::Kind => {
            let cluster = profile.image.cluster.clone().unwrap_or_else(|| "kind".to_owned());
            tool("kind", &["load", "docker-image", reference, "--name", &cluster]).map(|_| ())
        }
        ImageLoad::Minikube => tool("minikube", &["image", "load", reference]).map(|_| ()),
        ImageLoad::Push => {
            if push_reference != reference {
                tool("docker", &["tag", reference, push_reference])?;
            }
            let _tunnel = Tunnel::open(profile, push_reference)?;
            tool("docker", &["push", push_reference]).map(|_| ())
        }
    }
}

/// The command that makes the push address reachable, alive only while the push needs it.
struct Tunnel {
    child: Option<std::process::Child>,
}

impl Tunnel {
    fn open(profile: &Profile, push_reference: &str) -> Result<Self> {
        let Some(command) = profile.image.push_tunnel.as_ref() else {
            return Ok(Self { child: None });
        };
        let Some((program, arguments)) = command.split_first() else {
            return Ok(Self { child: None });
        };
        let child = Command::new(program)
            .args(arguments)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| {
                err(
                    codes::ENV_IMAGE_BUILD_FAILED,
                    format!("failed to start the push tunnel ({program}): {error}"),
                )
            })?;
        let tunnel = Self { child: Some(child) };
        let address = push_reference.split('/').next().unwrap_or_default().to_owned();
        tunnel.wait_for(&address)?;
        Ok(tunnel)
    }

    /// The tunnel is up when the address answers, not when the command that opens it has been
    /// started.
    fn wait_for(&self, address: &str) -> Result<()> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if std::net::TcpStream::connect(address).is_ok() {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(err(
                    codes::ENV_IMAGE_BUILD_FAILED,
                    format!("the push tunnel never made {address} reachable"),
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn inspect_id(reference: &str) -> Result<Option<String>> {
    let output = Command::new("docker")
        .args(["image", "inspect", "--format", "{{.Id}}", reference])
        .output();
    let Ok(output) = output else {
        return Err(err(
            codes::ENV_IMAGE_BUILD_FAILED,
            "docker is required to identify the image but could not be executed",
        ));
    };
    if !output.status.success() {
        return Ok(None);
    }
    let id = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok((!id.is_empty()).then_some(id))
}

/// Shells out the way the rest of this CLI shells out to git: inherit nothing, capture
/// everything, and turn a non-zero exit into a named failure.
fn tool(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program).args(args).output().map_err(|error| {
        err(
            codes::ENV_IMAGE_BUILD_FAILED,
            format!("failed to execute {program}: {error}"),
        )
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() { stdout } else { stderr };
        return Err(err(
            codes::ENV_IMAGE_BUILD_FAILED,
            format!(
                "{program} {} failed (exit {}): {}",
                args.join(" "),
                output.status.code().unwrap_or(-1),
                detail.trim()
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worktree_with(body: &str) -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        let devcontainer = directory.path().join(DEVCONTAINER_DIR);
        std::fs::create_dir_all(devcontainer.join("nested")).unwrap();
        std::fs::write(devcontainer.join("devcontainer.json"), body).unwrap();
        std::fs::write(devcontainer.join("nested/Dockerfile"), "FROM scratch\n").unwrap();
        directory
    }

    #[test]
    fn the_hash_covers_everything_the_container_is_built_from() {
        let first = worktree_with("{\"image\":\"a\"}");
        let same = source_hash(first.path()).unwrap();
        assert_eq!(same, source_hash(first.path()).unwrap(), "hashing is stable");

        std::fs::write(
            first.path().join(DEVCONTAINER_DIR).join("nested/Dockerfile"),
            "FROM scratch\nRUN true\n",
        )
        .unwrap();
        assert_ne!(
            same,
            source_hash(first.path()).unwrap(),
            "a file below the top level still changes the environment"
        );

        let second = worktree_with("{\"image\":\"b\"}");
        assert_ne!(same, source_hash(second.path()).unwrap());
    }

    #[test]
    fn a_repository_with_no_container_definition_says_so() {
        let directory = tempfile::tempdir().unwrap();
        let error = source_hash(directory.path()).unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<crate::error::CliFailure>()
                .map(|failure| failure.code.as_str()),
            Some(codes::ENV_IMAGE_BUILD_FAILED)
        );
    }
}
