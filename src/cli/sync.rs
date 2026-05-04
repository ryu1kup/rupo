use std::path::{Path, PathBuf};

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

    // Use depth from config (set at init time)
    let opts = SyncOptions {
        depth: config.depth,
        ..opts
    };

    let result: SyncResult = parallel::run(work_dir, &manifest, &opts).await;

    // Apply linkfiles and copyfiles for successfully synced projects
    apply_file_links(work_dir, &manifest, &result);

    print_result(&result);
    Ok(())
}

/// Create symlinks (linkfile) and copy files (copyfile) declared in the manifest.
///
/// Only applied for projects that synced successfully.
fn apply_file_links(
    work_dir: &Path,
    manifest: &manifest_toml::Manifest,
    result: &SyncResult,
) {
    for project in &manifest.projects {
        if !result.success.contains(&project.path) {
            continue;
        }

        let project_dir = work_dir.join(&project.path);

        for lf in &project.linkfiles {
            let src = project_dir.join(&lf.src);
            let dest = work_dir.join(&lf.dest);
            if let Err(e) = create_symlink(&src, &dest) {
                eprintln!("linkfile {}: {e}", lf.dest.display());
            }
        }

        for cf in &project.copyfiles {
            let src = project_dir.join(&cf.src);
            let dest = work_dir.join(&cf.dest);
            if let Err(e) = copy_file(&src, &dest) {
                eprintln!("copyfile {}: {e}", cf.dest.display());
            }
        }
    }
}

/// Create a symlink at `dest` pointing to `src`.
///
/// The symlink target is stored as a relative path from the dest's parent directory.
fn create_symlink(src: &Path, dest: &Path) -> Result<()> {
    let dest_parent = dest.parent().context("dest has no parent")?;
    let rel = relative_path(dest_parent, src);

    // Remove existing dest (symlink or file) so we can recreate
    if dest.exists() || dest.symlink_metadata().is_ok() {
        std::fs::remove_file(dest)
            .with_context(|| format!("failed to remove existing {}", dest.display()))?;
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    #[cfg(unix)]
    std::os::unix::fs::symlink(&rel, dest)
        .with_context(|| format!("symlink {} -> {}", dest.display(), rel.display()))?;

    #[cfg(not(unix))]
    std::fs::copy(src, dest)
        .with_context(|| format!("copy {} -> {}", src.display(), dest.display()))?;

    Ok(())
}

/// Compute a relative path from `base` to `target`.
///
/// Both paths should share a common ancestor (e.g. workspace root).
fn relative_path(base: &Path, target: &Path) -> PathBuf {
    use std::path::Component;

    let base_parts: Vec<_> = base.components().collect();
    let target_parts: Vec<_> = target.components().collect();

    // Find length of common prefix
    let common = base_parts
        .iter()
        .zip(target_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let mut rel = PathBuf::new();
    // Go up from base to common ancestor
    for _ in &base_parts[common..] {
        if matches!(base_parts[common], Component::Normal(_) | Component::CurDir) || common < base_parts.len() {
            rel.push("..");
        }
    }
    // Descend to target
    for part in &target_parts[common..] {
        rel.push(part);
    }
    rel
}

/// Copy a file from `src` to `dest`.
fn copy_file(src: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(src, dest)
        .with_context(|| format!("copy {} -> {}", src.display(), dest.display()))?;
    Ok(())
}

fn print_result(result: &SyncResult) {
    for path in &result.success {
        println!("✓ {}", path.display());
    }
    for (path, err) in &result.failure {
        println!("✗ {}: {err}", path.display());
    }

    let total = result.success.len() + result.failure.len();
    println!("Synced {}/{} projects", result.success.len(), total);
}
