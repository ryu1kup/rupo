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

const UPDATED_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest>
  <remote name="origin" fetch="https://github.com/example" />
  <default revision="main" remote="origin" />
  <project path="app/core" name="core" />
  <project path="app/ui" name="ui" revision="dev" />
  <project path="app/new" name="new-module" />
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

/// Push an updated `default.xml` to the bare manifest repository.
fn update_manifest_in_repo(parent: &Path, content: &str) {
    let work_dir = parent.join("manifest-work");
    let bare_repo = parent.join("manifest.git");
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
            "update manifest",
        ])
        .current_dir(&work_dir)
        .output()
        .unwrap();
    let output = Command::new("git")
        .arg("push")
        .arg(&bare_repo)
        .arg("main")
        .current_dir(&work_dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git push failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn init_with_valid_manifest_creates_workspace() {
    let tmp = TempDir::new().unwrap();
    let url = create_manifest_repo(tmp.path(), SAMPLE_MANIFEST);
    let work_dir = tmp.path().join("project");
    std::fs::create_dir(&work_dir).unwrap();

    rupo::cli::init::run(&url, Some("main"), "default.xml", None, None, work_dir.as_path())
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

    // Verify config includes default groups
    let config_content = std::fs::read_to_string(workspace.join("config.toml")).unwrap();
    assert!(
        !config_content.contains("groups"),
        "groups should not appear when None"
    );
}

#[tokio::test]
async fn init_with_branch_override_sets_revision_in_toml() {
    let tmp = TempDir::new().unwrap();
    let url = create_manifest_repo(tmp.path(), SAMPLE_MANIFEST);
    let work_dir = tmp.path().join("project");
    std::fs::create_dir(&work_dir).unwrap();

    rupo::cli::init::run(&url, Some("main"), "default.xml", None, None, work_dir.as_path())
        .await
        .unwrap();

    let toml_content = std::fs::read_to_string(work_dir.join(".rupo").join("rupo.toml")).unwrap();
    assert!(
        toml_content.contains(r#"revision = "main""#),
        "expected branch override in TOML, got:\n{toml_content}"
    );
}

#[tokio::test]
async fn init_with_corrupted_workspace_returns_error() {
    let tmp = TempDir::new().unwrap();
    let work_dir = tmp.path().join("project");
    // Create .rupo/ without a valid manifests git repo
    std::fs::create_dir_all(work_dir.join(".rupo")).unwrap();

    let result: anyhow::Result<()> = rupo::cli::init::run(
        "/nonexistent",
        None::<&str>,
        "default.xml",
        None,
        None,
        work_dir.as_path(),
    )
    .await;

    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("failed to fetch"),
        "expected fetch error, got: {msg}"
    );
}

#[tokio::test]
async fn init_with_invalid_url_returns_error() {
    let tmp = TempDir::new().unwrap();
    let work_dir = tmp.path().join("project");
    std::fs::create_dir(&work_dir).unwrap();

    let result: anyhow::Result<()> = rupo::cli::init::run(
        "://bad-url",
        None::<&str>,
        "default.xml",
        None,
        None,
        work_dir.as_path(),
    )
    .await;

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

    rupo::cli::init::run(&url, Some("main"), "default.xml", None, None, work_dir.as_path())
        .await
        .unwrap();

    let toml_content = std::fs::read_to_string(work_dir.join(".rupo").join("rupo.toml")).unwrap();
    assert!(toml_content.contains(r#"path = "app/core""#));
    assert!(toml_content.contains(r#"path = "app/ui""#));
}

#[tokio::test]
async fn reinit_with_existing_workspace_fetches_and_updates_toml() {
    let tmp = TempDir::new().unwrap();
    let url = create_manifest_repo(tmp.path(), SAMPLE_MANIFEST);
    let work_dir = tmp.path().join("project");
    std::fs::create_dir(&work_dir).unwrap();

    // First init
    rupo::cli::init::run(&url, Some("main"), "default.xml", None, None, work_dir.as_path())
        .await
        .unwrap();

    let toml_before = std::fs::read_to_string(work_dir.join(".rupo").join("rupo.toml")).unwrap();
    assert!(
        !toml_before.contains(r#"name = "new-module""#),
        "new-module should not exist before reinit"
    );

    // Push updated manifest to the bare repo
    update_manifest_in_repo(tmp.path(), UPDATED_MANIFEST);

    // Reinit
    rupo::cli::init::run(&url, Some("main"), "default.xml", None, None, work_dir.as_path())
        .await
        .unwrap();

    let toml_after = std::fs::read_to_string(work_dir.join(".rupo").join("rupo.toml")).unwrap();
    assert!(
        toml_after.contains(r#"name = "new-module""#),
        "expected new-module after reinit, got:\n{toml_after}"
    );
    assert!(
        toml_after.contains(r#"path = "app/new""#),
        "expected app/new path after reinit, got:\n{toml_after}"
    );
}
