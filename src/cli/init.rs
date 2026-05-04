use std::path::Path;

use anyhow::{Context, Result};
use tokio::fs;

use crate::config::Config;
use crate::git::ops::{self, CloneOptions, FetchOptions};
use crate::manifest;

/// Initialize (or reinitialize) a rupo workspace from a manifest repository.
///
/// ## New init (`.rupo/` does not exist)
/// 1. Clone manifest repo into `.rupo/manifests/`
/// 2. Read `<manifest>` from the cloned repo
/// 3. Parse manifest XML → convert to `rupo.toml` → save
/// 4. Save `config.toml` with init parameters
///
/// ## Reinit (`.rupo/` already exists)
/// 1. Fetch and update `.rupo/manifests/`
/// 2. Re-read `<manifest>` from the updated repo
/// 3. Parse manifest XML → overwrite `rupo.toml`
/// 4. Overwrite `config.toml` with current parameters
pub async fn run(
    url: &str,
    branch: Option<&str>,
    manifest: &str,
    groups: Option<&str>,
    work_dir: &Path,
) -> Result<()> {
    let workspace = work_dir.join(".rupo");

    if workspace.exists() {
        reinitialize(&workspace, url, branch, manifest).await?;
        save_config(&workspace, url, branch, manifest, groups)?;
        println!("Reinitialized rupo workspace in {}", workspace.display());
    } else {
        fs::create_dir_all(&workspace)
            .await
            .context("failed to create .rupo directory")?;

        match initialize(&workspace, url, branch, manifest).await {
            Ok(()) => {
                save_config(&workspace, url, branch, manifest, groups)?;
                println!("Initialized rupo workspace in {}", workspace.display());
            }
            Err(e) => {
                // Best-effort cleanup on first-time init failure
                let _ = fs::remove_dir_all(&workspace).await;
                return Err(e);
            }
        }
    }

    Ok(())
}

/// Clone manifest repo and generate rupo.toml (first-time init).
async fn initialize(
    workspace: &Path,
    url: &str,
    branch: Option<&str>,
    manifest: &str,
) -> Result<()> {
    let manifests_dir = workspace.join("manifests");
    let clone_opts = CloneOptions {
        branch: branch.map(String::from),
        ..Default::default()
    };
    ops::clone(url, &manifests_dir, &clone_opts)
        .await
        .context("failed to clone manifest repository")?;

    parse_and_save(workspace, manifest, branch).await
}

/// Fetch updates from remote and regenerate rupo.toml (reinit).
async fn reinitialize(
    workspace: &Path,
    _url: &str,
    branch: Option<&str>,
    manifest: &str,
) -> Result<()> {
    let manifests_dir = workspace.join("manifests");
    let fetch_opts = FetchOptions {
        branch: branch.map(String::from),
        ..Default::default()
    };
    ops::fetch(&manifests_dir, &fetch_opts)
        .await
        .context("failed to fetch manifest repository")?;

    ops::reset_to_remote(&manifests_dir, branch)
        .await
        .context("failed to update manifest working tree")?;

    parse_and_save(workspace, manifest, branch).await
}

/// Read manifest XML, parse it, convert to rupo.toml, and save.
async fn parse_and_save(workspace: &Path, manifest: &str, branch: Option<&str>) -> Result<()> {
    let manifest_path = workspace.join("manifests").join(manifest);
    let content = fs::read_to_string(&manifest_path)
        .await
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;

    let xml_manifest = manifest::xml::parse(&content).context("failed to parse manifest.xml")?;

    let toml_manifest = manifest::toml::Manifest::from_xml(&xml_manifest, branch);
    let toml_content =
        toml::to_string_pretty(&toml_manifest).context("failed to serialize rupo.toml")?;
    fs::write(workspace.join("rupo.toml"), &toml_content)
        .await
        .context("failed to write rupo.toml")?;

    Ok(())
}

/// Persist init parameters to config.toml.
fn save_config(
    workspace: &Path,
    url: &str,
    branch: Option<&str>,
    manifest: &str,
    groups: Option<&str>,
) -> Result<()> {
    let config = Config {
        url: url.to_string(),
        branch: branch.map(String::from),
        manifest: manifest.to_string(),
        mirror: false,
        groups: groups.map(String::from),
    };
    config.save(workspace).context("failed to save config.toml")
}
