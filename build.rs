// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

// Embeds `plugins/example/` into the binary (ticket Architecture: "the example plugin's tree
// is embedded at build time"). `plugins/example/` is F6's to write; until it lands this simply
// embeds nothing, which keeps the crate building and keeps the embedded-vs-repository hash
// check (in `src/plugins/embedded.rs`) trivially equal on both sides — a genuine `plugins/example/`
// tree activates the same mechanism without any change here.

use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let example_dir = manifest_dir.join("plugins").join("example");
    println!("cargo:rerun-if-changed={}", example_dir.display());

    let mut files = Vec::new();
    if example_dir.is_dir() {
        collect_files(&example_dir, &example_dir, &mut files);
        files.sort();
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let dest = out_dir.join("embedded_example.rs");

    let mut body = String::new();
    body.push_str("&[\n");
    for relative in &files {
        let full = example_dir.join(relative);
        let mode = file_mode(&full);
        let relative_str = relative.to_string_lossy().replace('\\', "/");
        body.push_str(&format!(
            "    crate::plugins::embedded::EmbeddedFile {{ relative_path: \"{relative_str}\", mode: {mode}, bytes: include_bytes!({full:?}) }},\n"
        ));
    }
    body.push_str("]\n");

    fs::write(&dest, body).expect("failed to write embedded example plugin source");
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out);
        } else if path.is_file() {
            out.push(path.strip_prefix(root).unwrap().to_path_buf());
        }
    }
}

#[cfg(unix)]
fn file_mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).map(|m| m.permissions().mode() & 0o777).unwrap_or(0o644)
}

#[cfg(not(unix))]
fn file_mode(_path: &Path) -> u32 {
    0o644
}
