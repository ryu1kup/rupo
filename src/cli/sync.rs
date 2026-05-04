use std::path::Path;

use anyhow::{Context, Result};

use crate::config::Config;
use crate::group::GroupFilter;
use crate::manifest::toml as manifest_toml;
use crate::sync::parallel::{self, SyncOptions, SyncResult};

/// Run `rupo sync`: clone or fetch every project listed in rupo.toml.
///
/// If `cli_groups` is provided, it overrides the groups saved in config.toml
/// for this sync only (config is not modified).
pub async fn run(work_dir: &Path, opts: SyncOptions, cli_groups: Option<&str>) -> Result<()> {
    let workspace = work_dir.join(".rupo");

    // Load saved configuration (written by `rupo init`)
    let config: Config = Config::load(&workspace)
        .context("failed to load .rupo/config.toml — run `rupo init` first")?;

    // Load manifest
    let toml_path = workspace.join("rupo.toml");
    let content = std::fs::read_to_string(&toml_path)
        .with_context(|| format!("failed to read {}", toml_path.display()))?;
    let mut manifest: manifest_toml::Manifest =
        manifest_toml::parse(&content).context("failed to parse rupo.toml")?;

    // Apply group filter: CLI overrides config, which defaults to "default"
    let group_str = cli_groups
        .map(String::from)
        .or(config.groups)
        .unwrap_or_else(|| "default".to_string());
    let filter = GroupFilter::parse(&group_str);
    let before = manifest.projects.len();
    manifest.projects.retain(|p| filter.matches(&p.groups));
    let after = manifest.projects.len();

    if before != after {
        println!(
            "Group filter ({group_str}): syncing {after}/{before} projects"
        );
    }

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
