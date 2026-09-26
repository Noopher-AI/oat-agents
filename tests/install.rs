use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn install_sh_honours_its_overrides() {
    let scratch = tempfile::tempdir().unwrap();
    let bin_dir = scratch.path().join("bin");
    let skill_root = scratch.path().join("skills");

    let status = Command::new("sh")
        .arg(repo_root().join("install.sh"))
        .arg("--bin-dir")
        .arg(&bin_dir)
        .arg("--skill-root")
        .arg(&skill_root)
        .status()
        .unwrap();
    assert!(status.success());

    assert!(bin_dir.join("oat-agents").exists());
    assert!(skill_root.join("oat-agents-cli/SKILL.md").exists());
}

#[test]
fn install_sh_skip_skill_installs_only_the_binary() {
    let scratch = tempfile::tempdir().unwrap();
    let bin_dir = scratch.path().join("bin");
    let skill_root = scratch.path().join("skills");

    let status = Command::new("sh")
        .arg(repo_root().join("install.sh"))
        .arg("--bin-dir")
        .arg(&bin_dir)
        .arg("--skill-root")
        .arg(&skill_root)
        .arg("--skip-skill")
        .status()
        .unwrap();
    assert!(status.success());

    assert!(bin_dir.join("oat-agents").exists());
    assert!(!skill_root.exists());
}

#[test]
fn install_sh_rejects_an_unknown_flag() {
    let status = Command::new("sh")
        .arg(repo_root().join("install.sh"))
        .arg("--bogus")
        .status()
        .unwrap();
    assert!(!status.success());
}
