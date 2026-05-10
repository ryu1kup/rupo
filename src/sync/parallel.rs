use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::{Semaphore, watch};
use tracing::{debug, info, warn};

use crate::git::ops::{self, CloneOptions, FetchOptions};
use crate::manifest::toml::Manifest;

/// Options controlling parallel sync behaviour.
pub struct SyncOptions {
    /// Number of parallel jobs.
    pub jobs: usize,
    /// Only sync the current branch.
    pub current_branch: bool,
    /// Shallow clone depth.
    pub depth: Option<u32>,
}

/// Outcome of a parallel sync run.
pub struct SyncResult {
    /// Project paths that synced successfully.
    pub success: Vec<PathBuf>,
    /// `(project_path, error_message)` for each failure.
    pub failure: Vec<(PathBuf, String)>,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            jobs: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4),
            current_branch: false,
            depth: None,
        }
    }
}

/// Strip trailing `/` so that path comparisons work uniformly regardless of
/// whether the manifest author included a trailing separator.
fn normalize_path(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    PathBuf::from(s.trim_end_matches('/'))
}

/// Sync every project listed in `manifest` under `work_dir`.
///
/// Each project is either cloned (if not yet present) or fetched (if already
/// present). Work is distributed across at most `opts.jobs` concurrent tasks.
/// A single project failure does **not** abort the remaining projects.
///
/// Projects whose paths are prefixes of other projects (parent-child
/// relationship) are ordered so that the parent is fully cloned before
/// any child starts, preventing "non-empty directory" errors. If a parent
/// project fails, its children are also marked as failed.
pub async fn run(work_dir: &Path, manifest: &Manifest, opts: &SyncOptions) -> SyncResult {
    let semaphore = Arc::new(Semaphore::new(opts.jobs));

    // Resolve the default remote name / revision once.
    let default_remote = manifest
        .defaults
        .as_ref()
        .and_then(|d| d.remote.clone())
        .unwrap_or_default();
    let default_revision = manifest.defaults.as_ref().and_then(|d| d.revision.clone());

    // Build a remote-name → fetch-url lookup.
    let remotes: HashMap<&str, &str> = manifest
        .remotes
        .iter()
        .map(|r| (r.name.as_str(), r.fetch.as_str()))
        .collect();

    // ---- Parent-child dependency tracking ----
    // Collect all *normalized* project paths (trailing `/` stripped) to
    // detect nesting correctly.
    let norm_paths: HashSet<PathBuf> = manifest
        .projects
        .iter()
        .map(|p| normalize_path(&p.path))
        .collect();

    // For each project, find its closest ancestor that is also a project.
    let mut parent_of: HashMap<PathBuf, PathBuf> = HashMap::new();
    for path in &norm_paths {
        let mut best: Option<&Path> = None;
        let path_str = path.to_string_lossy();
        for candidate in &norm_paths {
            let cand_str = candidate.to_string_lossy();
            if candidate != path
                && path_str.starts_with(cand_str.as_ref())
                && path_str.as_bytes().get(cand_str.len()) == Some(&b'/')
                && best.is_none_or(|b| candidate.as_os_str().len() > b.as_os_str().len())
            {
                best = Some(candidate.as_path());
            }
        }
        if let Some(p) = best {
            parent_of.insert(path.clone(), p.to_owned());
        }
    }

    // Create completion signals for projects that have children.
    // The channel carries `Option<bool>`: `None` = not done, `Some(true)`
    // = parent succeeded, `Some(false)` = parent failed.
    let parent_paths: HashSet<&PathBuf> = parent_of.values().collect();
    let mut done_txs: HashMap<PathBuf, watch::Sender<Option<bool>>> = HashMap::new();
    let mut done_rxs: HashMap<PathBuf, watch::Receiver<Option<bool>>> = HashMap::new();
    for path in &parent_paths {
        let (tx, rx) = watch::channel(None);
        done_txs.insert((*path).clone(), tx);
        done_rxs.insert((*path).clone(), rx);
    }

    // ---- Spawn tasks ----
    let mut handles = Vec::with_capacity(manifest.projects.len());

    for project in &manifest.projects {
        let remote_name = project.remote.as_deref().unwrap_or(&default_remote);
        let fetch_url = remotes.get(remote_name).copied().unwrap_or("");
        let url = format!("{}/{}", fetch_url.trim_end_matches('/'), project.name);

        let revision = project
            .revision
            .clone()
            .or_else(|| default_revision.clone());

        let target_path: PathBuf = work_dir.join(&project.path);
        let project_path = project.path.clone();
        let norm = normalize_path(&project.path);
        let depth = opts.depth;
        let current_branch = opts.current_branch;
        let sem = Arc::clone(&semaphore);

        // If this project has a parent project, grab a receiver to wait on.
        let wait_rx = parent_of
            .get(&norm)
            .and_then(|pp| done_rxs.get(pp).cloned());

        // If this project is a parent, grab the sender to signal completion.
        let done_tx = done_txs.remove(&norm);

        handles.push(tokio::spawn(async move {
            // Wait for parent project to finish before starting.
            if let Some(mut rx) = wait_rx {
                let _ = rx.wait_for(|v| v.is_some()).await;
                // If parent failed, fail immediately.
                if *rx.borrow() == Some(false) {
                    let err = anyhow::anyhow!("skipped: parent project failed");
                    warn!(project = %project_path.display(), "skipped: parent failed");
                    if let Some(tx) = done_tx {
                        let _ = tx.send(Some(false));
                    }
                    return (project_path, Err(err));
                }
            }

            let _permit = sem.acquire().await.expect("semaphore should not be closed");

            debug!(project = %project_path.display(), "sync started");
            let start = std::time::Instant::now();

            let result = sync_one_project(
                &url,
                &target_path,
                revision.as_deref(),
                depth,
                current_branch,
            )
            .await;

            let elapsed = start.elapsed();

            match &result {
                Ok(()) => info!(
                    project = %project_path.display(),
                    elapsed_ms = elapsed.as_millis() as u64,
                    "sync ok"
                ),
                Err(e) => warn!(
                    project = %project_path.display(),
                    elapsed_ms = elapsed.as_millis() as u64,
                    error = %e,
                    "sync failed"
                ),
            }

            // Signal children: success or failure.
            if let Some(tx) = done_tx {
                let _ = tx.send(Some(result.is_ok()));
            }

            (project_path, result)
        }));
    }

    let mut success = Vec::new();
    let mut failure = Vec::new();

    for handle in handles {
        match handle.await {
            Ok((path, Ok(()))) => success.push(path),
            Ok((path, Err(e))) => failure.push((path, format!("{e:#}"))),
            Err(e) => failure.push((PathBuf::from("unknown"), format!("task panicked: {e}"))),
        }
    }

    SyncResult { success, failure }
}

/// Clone or fetch a single project.
async fn sync_one_project(
    url: &str,
    target_path: &Path,
    revision: Option<&str>,
    depth: Option<u32>,
    _current_branch: bool,
) -> Result<()> {
    let clone_opts = CloneOptions {
        depth,
        branch: revision.map(String::from),
    };

    if target_path.join(".git").exists() {
        // Already cloned – fetch updates.
        let fetch_opts = FetchOptions {
            depth,
            branch: revision.map(String::from),
        };
        ops::fetch(target_path, &fetch_opts)
            .await
            .with_context(|| format!("fetch failed for {}", target_path.display()))?;

        ops::reset_to_remote(target_path, revision)
            .await
            .with_context(|| format!("reset failed for {}", target_path.display()))?;
    } else if target_path.exists() {
        // Directory exists but is not a git repo (e.g. created by a
        // previously-cloned child project). Initialise in-place.
        ops::init_and_fetch(url, target_path, &clone_opts)
            .await
            .with_context(|| format!("init-and-fetch failed for {}", target_path.display()))?;
    } else {
        // Fresh clone.
        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        ops::clone(url, target_path, &clone_opts)
            .await
            .with_context(|| format!("clone failed for {}", target_path.display()))?;
    }

    Ok(())
}
