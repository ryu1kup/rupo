use std::path::Path;

use anyhow::{Context, Result, bail};
use tokio::fs;

use crate::manifest;

/// Initialize a rupo workspace from a manifest URL.
pub async fn run(url: &str, branch: Option<&str>, work_dir: &Path) -> Result<()> {
    let workspace = work_dir.join(".rupo");

    if workspace.exists() {
        bail!("workspace already initialized at {}", workspace.display());
    }

    // Fetch manifest.xml
    let response = reqwest::get(url)
        .await
        .context("failed to fetch manifest")?;
    let content = response
        .error_for_status()
        .context("manifest fetch returned an error status")?
        .text()
        .await
        .context("failed to read manifest response body")?;

    // Parse
    let xml_manifest = manifest::xml::parse(&content).context("failed to parse manifest.xml")?;

    // Create .rupo/
    fs::create_dir_all(&workspace)
        .await
        .context("failed to create .rupo directory")?;

    // Cache manifest.xml
    fs::write(workspace.join("manifest.xml"), &content)
        .await
        .context("failed to cache manifest.xml")?;

    // Convert to rupo.toml and save
    let toml_manifest = manifest::toml::Manifest::from_xml(&xml_manifest, branch);
    let toml_content =
        toml::to_string_pretty(&toml_manifest).context("failed to serialize rupo.toml")?;
    fs::write(workspace.join("rupo.toml"), &toml_content)
        .await
        .context("failed to write rupo.toml")?;

    println!("Initialized rupo workspace in {}", workspace.display());
    Ok(())
}
