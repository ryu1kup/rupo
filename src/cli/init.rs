use std::path::Path;

use anyhow::{Context, Result, bail};
use tokio::fs;

use crate::git::ops::{self, CloneOptions};
use crate::manifest;

const DEFAULT_MANIFEST_FILE: &str = "default.xml";

/// Initialize a rupo workspace from a manifest repository URL.
///
/// Flow:
/// 1. Bail if `.rupo/` already exists
/// 2. Clone manifest repo into `.rupo/manifests/`
/// 3. Read `default.xml` from the cloned repo
/// 4. Parse manifest XML
/// 5. Convert to `rupo.toml` and save in `.rupo/`
pub async fn run(url: &str, branch: Option<&str>, work_dir: &Path) -> Result<()> {
    let workspace = work_dir.join(".rupo");

    if workspace.exists() {
        bail!("workspace already initialized at {}", workspace.display());
    }

    // Create workspace directory
    fs::create_dir_all(&workspace)
        .await
        .context("failed to create .rupo directory")?;

    // Run the rest of init; clean up .rupo/ on failure.
    match init_workspace(&workspace, url, branch).await {
        Ok(()) => {
            println!("Initialized rupo workspace in {}", workspace.display());
            Ok(())
        }
        Err(e) => {
            // Best-effort cleanup
            let _ = fs::remove_dir_all(&workspace).await;
            Err(e)
        }
    }
}

async fn init_workspace(workspace: &Path, url: &str, branch: Option<&str>) -> Result<()> {
    // Clone manifest repository
    let manifests_dir = workspace.join("manifests");
    let clone_opts = CloneOptions {
        branch: branch.map(String::from),
        ..Default::default()
    };
    ops::clone(url, &manifests_dir, &clone_opts)
        .await
        .context("failed to clone manifest repository")?;

    // Read manifest XML
    // TODO: support -m option to specify manifest filename
    let manifest_path = manifests_dir.join(DEFAULT_MANIFEST_FILE);
    let content = fs::read_to_string(&manifest_path)
        .await
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;

    // Parse
    let xml_manifest = manifest::xml::parse(&content).context("failed to parse manifest.xml")?;

    // Convert to rupo.toml and save
    let toml_manifest = manifest::toml::Manifest::from_xml(&xml_manifest, branch);
    let toml_content =
        toml::to_string_pretty(&toml_manifest).context("failed to serialize rupo.toml")?;
    fs::write(workspace.join("rupo.toml"), &toml_content)
        .await
        .context("failed to write rupo.toml")?;

    Ok(())
}
