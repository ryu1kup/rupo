use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const SAMPLE_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest>
  <remote name="origin" fetch="https://github.com/example" />
  <default revision="main" remote="origin" />
  <project path="app/core" name="core" />
  <project path="app/ui" name="ui" revision="dev" />
</manifest>"#;

/// Start a one-shot HTTP server that responds with the given body.
async fn serve_once(body: &str) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}/manifest.xml");
    let body = body.to_string();

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 4096];
        let _ = stream.read(&mut buf).await.unwrap();

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.shutdown().await.unwrap();
    });

    (url, handle)
}

#[tokio::test]
async fn init_with_valid_manifest_creates_workspace() {
    let tmp = TempDir::new().unwrap();
    let (url, server) = serve_once(SAMPLE_MANIFEST).await;

    rupo::cli::init::run(&url, None::<&str>, tmp.path())
        .await
        .unwrap();
    server.await.unwrap();

    let workspace = tmp.path().join(".rupo");
    assert!(workspace.join("manifest.xml").exists());
    assert!(workspace.join("rupo.toml").exists());

    // Verify cached XML
    let xml = std::fs::read_to_string(workspace.join("manifest.xml")).unwrap();
    assert!(xml.contains(r#"<remote name="origin""#));

    // Verify generated TOML
    let toml_content = std::fs::read_to_string(workspace.join("rupo.toml")).unwrap();
    assert!(toml_content.contains(r#"name = "origin""#));
    assert!(toml_content.contains(r#"name = "core""#));
    assert!(toml_content.contains(r#"revision = "main""#));
}

#[tokio::test]
async fn init_with_branch_override_sets_revision_in_toml() {
    let tmp = TempDir::new().unwrap();
    let (url, server) = serve_once(SAMPLE_MANIFEST).await;

    rupo::cli::init::run(&url, Some("develop"), tmp.path())
        .await
        .unwrap();
    server.await.unwrap();

    let toml_content = std::fs::read_to_string(tmp.path().join(".rupo").join("rupo.toml")).unwrap();
    assert!(
        toml_content.contains(r#"revision = "develop""#),
        "expected branch override in TOML, got:\n{toml_content}"
    );
}

#[tokio::test]
async fn init_with_existing_workspace_returns_error() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join(".rupo")).unwrap();

    let result: anyhow::Result<()> =
        rupo::cli::init::run("http://127.0.0.1:1/manifest.xml", None::<&str>, tmp.path()).await;

    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("already initialized"),
        "expected 'already initialized', got: {msg}"
    );
}

#[tokio::test]
async fn init_with_unreachable_url_returns_error() {
    let tmp = TempDir::new().unwrap();

    let result: anyhow::Result<()> =
        rupo::cli::init::run("http://127.0.0.1:1/manifest.xml", None::<&str>, tmp.path()).await;

    assert!(result.is_err());
    // .rupo/ should NOT be created on failure
    assert!(!tmp.path().join(".rupo").exists());
}

#[tokio::test]
async fn init_preserves_project_paths_as_pathbuf() {
    let tmp = TempDir::new().unwrap();
    let (url, server) = serve_once(SAMPLE_MANIFEST).await;

    rupo::cli::init::run(&url, None::<&str>, tmp.path())
        .await
        .unwrap();
    server.await.unwrap();

    let toml_content = std::fs::read_to_string(tmp.path().join(".rupo").join("rupo.toml")).unwrap();
    // Paths from XML should appear in TOML
    assert!(toml_content.contains(r#"path = "app/core""#));
    assert!(toml_content.contains(r#"path = "app/ui""#));
}
