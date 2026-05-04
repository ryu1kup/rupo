use std::path::Path;

use anyhow::{Context, Result};

use crate::config::Config;
use crate::manifest::toml as manifest_toml;
use crate::sync::parallel::{self, SyncOptions, SyncResult};

/// Run `rupo sync`: clone or fetch every project listed in rupo.toml.
pub async fn run(work_dir: &Path, opts: SyncOptions) -> Result<()> {
    let workspace = work_dir.join(".rupo");

    // Load saved configuration (written by `rupo init`)
    let _config: Config = Config::load(&workspace)
        .context("failed to load .rupo/config.toml — run `rupo init` first")?;

    // Load manifest
    let toml_path = workspace.join("rupo.toml");
    let content = std::fs::read_to_string(&toml_path)
        .with_context(|| format!("failed to read {}", toml_path.display()))?;
    let manifest: manifest_toml::Manifest =
        manifest_toml::parse(&content).context("failed to parse rupo.toml")?;

    let result: SyncResult = parallel::run(work_dir, &manifest, &opts).await;

    print_result(&result);
    Ok(())
}

fn print_result(result: &SyncResult) {
    for path in &result.success {
        println!("✓ {path}");
    }
    for (path, err) in &result.failure {
        println!("✗ {path}: {err}");
    }

    let total = result.success.len() + result.failure.len();
    println!("Synced {}/{} projects", result.success.len(), total);
}
