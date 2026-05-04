use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

const SAMPLE_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest>
  <remote name="origin" fetch="https://github.com/example" />
  <default revision="main" remote="origin" />
  <project path="app/core" name="core" />
  <project path="app/ui" name="ui" revision="dev" />
</manifest>"#;

/// Create a bare git repository containing `default.xml` with the given content.
/// Returns the path to the bare repo (usable as a clone URL).
fn create_manifest_repo(parent: &Path, content: &str) -> String {
    let repo_dir = parent.join("manifest.git");

    // Create a temporary working directory, commit, then clone --bare
    let work_dir = parent.join("manifest-work");
    Command::new("git")
        .args(["init", "--initial-branch=main"])
        .arg(&work_dir)
        .output()
        .unwrap();
    std::fs::write(work_dir.join("default.xml"), content).unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&work_dir)
        .output()
        .unwrap();
    Command::new("git")
        .args([
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@test.com",
            "commit",
            "-m",
            "init",
        ])
        .current_dir(&work_dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["clone", "--bare"])
        .arg(&work_dir)
        .arg(&repo_dir)
        .output()
        .unwrap();

    repo_dir.to_string_lossy().into_owned()
}

#[tokio::test]
async fn init_with_valid_manifest_creates_workspace() {
    let tmp = TempDir::new().unwrap();
    let url = create_manifest_repo(tmp.path(), SAMPLE_MANIFEST);
    let work_dir = tmp.path().join("project");
    std::fs::create_dir(&work_dir).unwrap();

    rupo::cli::init::run(&url, Some("main"), work_dir.as_path())
        .await
        .unwrap();

    let workspace = work_dir.join(".rupo");
    assert!(workspace.join("manifests").join("default.xml").exists());
    assert!(workspace.join("rupo.toml").exists());

    // Verify generated TOML
    let toml_content = std::fs::read_to_string(workspace.join("rupo.toml")).unwrap();
    assert!(toml_content.contains(r#"name = "origin""#));
    assert!(toml_content.contains(r#"name = "core""#));
    assert!(toml_content.contains(r#"revision = "main""#));
}

#[tokio::test]
async fn init_with_branch_override_sets_revision_in_toml() {
    let tmp = TempDir::new().unwrap();
    let url = create_manifest_repo(tmp.path(), SAMPLE_MANIFEST);
    let work_dir = tmp.path().join("project");
    std::fs::create_dir(&work_dir).unwrap();

    rupo::cli::init::run(&url, Some("main"), work_dir.as_path())
        .await
        .unwrap();

    let toml_content = std::fs::read_to_string(work_dir.join(".rupo").join("rupo.toml")).unwrap();
    assert!(
        toml_content.contains(r#"revision = "main""#),
        "expected branch override in TOML, got:\n{toml_content}"
    );
}

#[tokio::test]
async fn init_with_existing_workspace_returns_error() {
    let tmp = TempDir::new().unwrap();
    let work_dir = tmp.path().join("project");
    std::fs::create_dir_all(work_dir.join(".rupo")).unwrap();

    let result: anyhow::Result<()> =
        rupo::cli::init::run("/nonexistent", None::<&str>, work_dir.as_path()).await;

    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("already initialized"),
        "expected 'already initialized', got: {msg}"
    );
}

#[tokio::test]
async fn init_with_invalid_url_returns_error() {
    let tmp = TempDir::new().unwrap();
    let work_dir = tmp.path().join("project");
    std::fs::create_dir(&work_dir).unwrap();

    let result: anyhow::Result<()> =
        rupo::cli::init::run("://bad-url", None::<&str>, work_dir.as_path()).await;

    assert!(result.is_err());
    // .rupo/ should NOT have manifests or rupo.toml on failure
    assert!(!work_dir.join(".rupo").join("rupo.toml").exists());
}

#[tokio::test]
async fn init_preserves_project_paths_in_toml() {
    let tmp = TempDir::new().unwrap();
    let url = create_manifest_repo(tmp.path(), SAMPLE_MANIFEST);
    let work_dir = tmp.path().join("project");
    std::fs::create_dir(&work_dir).unwrap();

    rupo::cli::init::run(&url, Some("main"), work_dir.as_path())
        .await
        .unwrap();

    let toml_content = std::fs::read_to_string(work_dir.join(".rupo").join("rupo.toml")).unwrap();
    assert!(toml_content.contains(r#"path = "app/core""#));
    assert!(toml_content.contains(r#"path = "app/ui""#));
}
