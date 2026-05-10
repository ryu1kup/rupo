//! Git operations abstraction layer.
//!
//! All Git I/O goes through this module so that the rest of the codebase
//! (cli/, sync/, etc.) never touches git directly.
//!
//! The public API is async. Blocking git subprocess calls are wrapped with
//! `tokio::task::spawn_blocking` internally so callers don't need to
//! manage the async/blocking boundary.
//!
//! Implementation: git CLI subprocess.

use std::path::Path;

use anyhow::{Context, Result};
use tracing::{debug, info, trace};

/// Options for [`clone`].
#[derive(Debug, Default, Clone)]
pub struct CloneOptions {
    /// Limit history depth (shallow clone). `None` means full history.
    pub depth: Option<u32>,
    /// Branch or revision to check out after cloning.
    pub branch: Option<String>,
}

/// Options for [`fetch`].
#[derive(Debug, Default, Clone)]
pub struct FetchOptions {
    /// Limit fetch depth. `None` means full history.
    pub depth: Option<u32>,
    /// Remote branch to fetch. `None` fetches the default refspec.
    pub branch: Option<String>,
}

// ---------------------------------------------------------------------------
// Public async API
// ---------------------------------------------------------------------------

/// Clone a repository from `url` into `target_path`.
pub async fn clone(url: &str, target_path: &Path, opts: &CloneOptions) -> Result<()> {
    let url = url.to_owned();
    let target_path = target_path.to_path_buf();
    let opts = opts.clone();

    tokio::task::spawn_blocking(move || clone_blocking(&url, &target_path, &opts))
        .await
        .context("clone task panicked")?
}

/// Fetch updates for an existing repository at `repo_path`.
pub async fn fetch(repo_path: &Path, opts: &FetchOptions) -> Result<()> {
    let repo_path = repo_path.to_path_buf();
    let opts = opts.clone();

    tokio::task::spawn_blocking(move || fetch_blocking(&repo_path, &opts))
        .await
        .context("fetch task panicked")?
}

/// Reset the working tree of a repository to match a remote branch.
///
/// After a [`fetch`], call this to update the local HEAD and working tree
/// to match `origin/<branch>`. Falls back to `@{upstream}` when `branch`
/// is `None`.
pub async fn reset_to_remote(repo_path: &Path, branch: Option<&str>) -> Result<()> {
    let repo_path = repo_path.to_path_buf();
    let branch = branch.map(String::from);

    tokio::task::spawn_blocking(move || reset_to_remote_blocking(&repo_path, branch.as_deref()))
        .await
        .context("reset task panicked")?
}

// ---------------------------------------------------------------------------
// Blocking implementations (private)
// ---------------------------------------------------------------------------

fn clone_blocking(url: &str, target_path: &Path, opts: &CloneOptions) -> Result<()> {
    info!(url, path = %target_path.display(), depth = opts.depth, branch = opts.branch.as_deref(), "cloning");

    let mut cmd = std::process::Command::new("git");
    cmd.arg("clone").arg("--single-branch").arg("--no-tags");

    if let Some(d) = opts.depth {
        cmd.arg(format!("--depth={d}"));
    }

    if let Some(ref b) = opts.branch {
        cmd.args(["--branch", b]);
    }

    cmd.arg(url).arg(target_path);

    trace!(cmd = ?cmd, "git clone");
    let output = cmd.output().context("failed to run git clone")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git clone failed: {}", stderr.trim());
    }

    Ok(())
}

fn fetch_blocking(repo_path: &Path, opts: &FetchOptions) -> Result<()> {
    info!(path = %repo_path.display(), depth = opts.depth, branch = opts.branch.as_deref(), "fetching");

    let mut args: Vec<&str> = vec!["fetch", "--no-tags"];

    let depth_flag;
    if let Some(d) = opts.depth {
        depth_flag = format!("--depth={d}");
        args.push(&depth_flag);
    }

    args.push("origin");

    if let Some(ref b) = opts.branch {
        args.push(b);
    }

    run_git(repo_path, &args)
}

fn reset_to_remote_blocking(repo_path: &Path, branch: Option<&str>) -> Result<()> {
    let target = match branch {
        Some(b) => format!("origin/{b}"),
        None => "@{upstream}".to_string(),
    };
    debug!(path = %repo_path.display(), target, "resetting to remote");

    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["reset", "--hard", &target])
        .output()
        .context("failed to run git reset")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git reset --hard {target} failed: {stderr}");
    }

    Ok(())
}

/// Initialize a directory as a git repo and fetch objects, but **do not**
/// checkout a working tree. Used in two-phase sync where checkout is deferred.
pub async fn init_and_fetch_only(url: &str, target_path: &Path, opts: &CloneOptions) -> Result<()> {
    let url = url.to_owned();
    let target_path = target_path.to_path_buf();
    let opts = opts.clone();

    tokio::task::spawn_blocking(move || {
        init_and_fetch_only_blocking(&url, &target_path, opts.branch.as_deref(), opts.depth)
    })
    .await
    .context("init_and_fetch_only task panicked")?
}

/// Checkout a branch after a fetch. Creates or resets a local branch
/// tracking the remote ref.
pub async fn checkout_branch(repo_path: &Path, branch: Option<&str>) -> Result<()> {
    let repo_path = repo_path.to_path_buf();
    let branch = branch.map(String::from);

    tokio::task::spawn_blocking(move || checkout_branch_blocking(&repo_path, branch.as_deref()))
        .await
        .context("checkout task panicked")?
}

fn init_and_fetch_only_blocking(
    url: &str,
    target: &Path,
    branch: Option<&str>,
    depth: Option<u32>,
) -> Result<()> {
    info!(url, path = %target.display(), branch, depth, "init-and-fetch-only");

    std::fs::create_dir_all(target)
        .with_context(|| format!("failed to create {}", target.display()))?;

    run_git(target, &["init"])?;

    if run_git(target, &["remote", "add", "origin", url]).is_err() {
        run_git(target, &["remote", "set-url", "origin", url])?;
    }

    let depth_flag;
    let mut args = vec!["fetch", "--no-tags"];
    if let Some(d) = depth {
        depth_flag = format!("--depth={d}");
        args.push(&depth_flag);
    }
    args.push("origin");
    if let Some(b) = branch {
        args.push(b);
    }
    run_git(target, &args)
}

fn checkout_branch_blocking(repo_path: &Path, branch: Option<&str>) -> Result<()> {
    debug!(path = %repo_path.display(), branch, "checking out branch");
    if let Some(b) = branch {
        let remote_ref = format!("origin/{b}");
        run_git(repo_path, &["checkout", "-B", b, &remote_ref])
    } else {
        run_git(repo_path, &["checkout", "FETCH_HEAD"])
    }
}

/// Run a `git` sub-command in `dir` and return an error on non-zero exit.
fn run_git(dir: &Path, args: &[&str]) -> Result<()> {
    trace!(dir = %dir.display(), ?args, "git");
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .with_context(|| format!("failed to run git {}", args.first().unwrap_or(&"")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "git {} failed: {}",
            args.first().unwrap_or(&""),
            stderr.trim()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone_options_default_has_no_depth_or_branch() {
        let opts = CloneOptions::default();
        assert!(opts.depth.is_none());
        assert!(opts.branch.is_none());
    }

    #[test]
    fn fetch_options_default_has_no_depth_or_branch() {
        let opts = FetchOptions::default();
        assert!(opts.depth.is_none());
        assert!(opts.branch.is_none());
    }

    #[test]
    fn clone_blocking_with_invalid_url_returns_error() {
        let target = std::env::temp_dir().join("rupo-test-nonexistent");
        let result = clone_blocking("://not-a-url", &target, &CloneOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn fetch_blocking_with_nonexistent_repo_returns_error() {
        let path = std::env::temp_dir().join("rupo-test-no-such-repo");
        let result = fetch_blocking(&path, &FetchOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn clone_options_with_depth_and_branch() {
        let opts = CloneOptions {
            depth: Some(1),
            branch: Some("main".into()),
        };
        assert_eq!(opts.depth, Some(1));
        assert_eq!(opts.branch.as_deref(), Some("main"));
    }

    #[tokio::test]
    async fn clone_async_with_invalid_url_returns_error() {
        let target = std::env::temp_dir().join("rupo-test-async-nonexistent");
        let result = clone("://not-a-url", &target, &CloneOptions::default()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn fetch_async_with_nonexistent_repo_returns_error() {
        let path = std::env::temp_dir().join("rupo-test-async-no-such-repo");
        let result = fetch(&path, &FetchOptions::default()).await;
        assert!(result.is_err());
    }
}
