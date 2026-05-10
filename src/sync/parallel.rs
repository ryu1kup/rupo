use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

use crate::git::ops::{self, CloneOptions, FetchOptions};
use crate::manifest::toml::{Manifest, SizeHint};
use crate::sync::stats::SyncStats;

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
    /// Per-project durations in milliseconds (for stats recording).
    pub durations: HashMap<String, u64>,
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

/// Two-phase sync: fetch all projects in parallel, then checkout in
/// parent→child order.
///
/// Phase 1 runs all network I/O (git fetch) concurrently with semaphore
/// limiting. No parent-child ordering is needed because fetch does not
/// write to the working tree.
///
/// Phase 2 runs checkout/reset sequentially in topological order (parents
/// before children) so that child repos correctly overwrite parent files.
pub async fn run(
    work_dir: &Path,
    manifest: &Manifest,
    opts: &SyncOptions,
    stats: &SyncStats,
) -> SyncResult {
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

    // ---- Parent-child detection ----
    let norm_paths: HashSet<PathBuf> = manifest
        .projects
        .iter()
        .map(|p| normalize_path(&p.path))
        .collect();

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

    // ---- Sort projects by estimated duration (largest first) ----
    let mut sorted_projects: Vec<_> = manifest.projects.iter().collect();
    sorted_projects.sort_by(|a, b| {
        let da = estimated_duration(a, stats);
        let db = estimated_duration(b, stats);
        db.cmp(&da) // descending
    });

    // ---- Phase 1: Parallel fetch (no parent-child ordering) ----
    info!(
        "phase 1: fetching {} projects in parallel",
        sorted_projects.len()
    );

    let mut fetch_handles = Vec::with_capacity(sorted_projects.len());

    for project in &sorted_projects {
        let remote_name = project.remote.as_deref().unwrap_or(&default_remote);
        let fetch_url = remotes.get(remote_name).copied().unwrap_or("");
        let url = format!("{}/{}", fetch_url.trim_end_matches('/'), project.name);
        let revision = project
            .revision
            .clone()
            .or_else(|| default_revision.clone());
        let target_path: PathBuf = work_dir.join(&project.path);
        let project_path = project.path.clone();
        let depth = opts.depth;
        let sem = Arc::clone(&semaphore);

        fetch_handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.expect("semaphore should not be closed");

            debug!(project = %project_path.display(), "fetch started");
            let start = std::time::Instant::now();

            let result = fetch_project(&url, &target_path, revision.as_deref(), depth).await;

            let elapsed_ms = start.elapsed().as_millis() as u64;
            match &result {
                Ok(existing) => info!(
                    project = %project_path.display(),
                    elapsed_ms,
                    existing,
                    "fetch ok"
                ),
                Err(e) => warn!(
                    project = %project_path.display(),
                    elapsed_ms,
                    error = %e,
                    "fetch failed"
                ),
            }

            (project_path, target_path, revision, result, elapsed_ms)
        }));
    }

    // Collect Phase 1 results.
    struct FetchedProject {
        project_path: PathBuf,
        target_path: PathBuf,
        revision: Option<String>,
        was_existing: bool,
    }

    let mut fetched: Vec<FetchedProject> = Vec::new();
    let mut failure: Vec<(PathBuf, String)> = Vec::new();
    let mut durations: HashMap<String, u64> = HashMap::new();

    for handle in fetch_handles {
        match handle.await {
            Ok((project_path, target_path, revision, Ok(was_existing), ms)) => {
                durations.insert(project_path.to_string_lossy().into_owned(), ms);
                fetched.push(FetchedProject {
                    project_path,
                    target_path,
                    revision,
                    was_existing,
                });
            }
            Ok((project_path, _, _, Err(e), _)) => {
                failure.push((project_path, format!("{e:#}")));
            }
            Err(e) => {
                failure.push((PathBuf::from("unknown"), format!("task panicked: {e}")));
            }
        }
    }

    // ---- Phase 2: Checkout in parent→child order ----
    info!("phase 2: checking out {} projects", fetched.len());

    // Sort by path depth (parents first).
    fetched.sort_by_key(|f| f.project_path.components().count());

    let failed_norms: HashSet<PathBuf> = failure.iter().map(|(p, _)| normalize_path(p)).collect();
    // Track checkout failures so children of failed checkouts are also skipped.
    let mut checkout_failed: HashSet<PathBuf> = HashSet::new();

    let mut success: Vec<PathBuf> = Vec::new();

    for proj in &fetched {
        let norm = normalize_path(&proj.project_path);

        // Skip if parent's fetch or checkout failed.
        if let Some(parent_norm) = parent_of.get(&norm)
            && (failed_norms.contains(parent_norm) || checkout_failed.contains(parent_norm))
        {
            warn!(project = %proj.project_path.display(), "skipped: parent failed");
            failure.push((
                proj.project_path.clone(),
                "skipped: parent project failed".into(),
            ));
            checkout_failed.insert(norm);
            continue;
        }

        let result = if proj.was_existing {
            ops::reset_to_remote(&proj.target_path, proj.revision.as_deref()).await
        } else {
            ops::checkout_branch(&proj.target_path, proj.revision.as_deref()).await
        };

        match result {
            Ok(()) => {
                info!(project = %proj.project_path.display(), "checkout ok");
                success.push(proj.project_path.clone());
            }
            Err(e) => {
                warn!(project = %proj.project_path.display(), error = %e, "checkout failed");
                failure.push((proj.project_path.clone(), format!("{e:#}")));
                checkout_failed.insert(norm);
            }
        }
    }

    SyncResult {
        success,
        failure,
        durations,
    }
}

/// Estimate sync duration for scheduling priority.
///
/// Resolution order: size-hint > sync-stats > default (medium).
fn estimated_duration(project: &crate::manifest::toml::ProjectEntry, stats: &SyncStats) -> u64 {
    if let Some(ref hint) = project.size_hint {
        return match hint {
            SizeHint::Large => 100_000,
            SizeHint::Medium => 10_000,
            SizeHint::Small => 1_000,
        };
    }
    let key = project.path.to_string_lossy();
    if let Some(s) = stats.projects.get(key.as_ref()) {
        return s.duration_ms;
    }
    10_000
}

/// Phase 1: Fetch a project without checkout.
/// Returns `Ok(true)` if the repo already existed (warm sync),
/// `Ok(false)` if freshly initialized (cold sync).
async fn fetch_project(
    url: &str,
    target_path: &Path,
    revision: Option<&str>,
    depth: Option<u32>,
) -> Result<bool> {
    if target_path.join(".git").exists() {
        // Warm sync: repo exists, just fetch.
        let fetch_opts = FetchOptions {
            depth,
            branch: revision.map(String::from),
        };
        ops::fetch(target_path, &fetch_opts)
            .await
            .with_context(|| format!("fetch failed for {}", target_path.display()))?;
        Ok(true)
    } else {
        // Cold sync: init + fetch (no checkout).
        let clone_opts = CloneOptions {
            depth,
            branch: revision.map(String::from),
        };
        ops::init_and_fetch_only(url, target_path, &clone_opts)
            .await
            .with_context(|| format!("init-and-fetch failed for {}", target_path.display()))?;
        Ok(false)
    }
}
