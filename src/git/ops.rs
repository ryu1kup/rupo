//! Git operations abstraction layer.
//!
//! All Git I/O goes through this module so that the rest of the codebase
//! (cli/, sync/, etc.) never touches `gix` directly.
//!
//! The public API is async. Blocking gix calls are wrapped with
//! `tokio::task::spawn_blocking` internally so callers don't need to
//! manage the async/blocking boundary.
//!
//! Current implementation: gix with the system `ssh` command for SSH transport.
//! TODO(native-ssh): Once gix's built-in SSH transport is stable, switch to
//! native SSH by changing the connection setup in this module alone.

use std::path::Path;

use anyhow::{Context, Result};

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
///
/// SSH URLs (e.g. `git@github.com:org/repo.git`) are supported through
/// the system `ssh` command.
///
/// TODO(native-ssh): Migrate to gix native SSH when stable.
pub async fn clone(url: &str, target_path: &Path, opts: &CloneOptions) -> Result<()> {
    let url = url.to_owned();
    let target_path = target_path.to_path_buf();
    let opts = opts.clone();

    tokio::task::spawn_blocking(move || clone_blocking(&url, &target_path, &opts))
        .await
        .context("clone task panicked")?
}

/// Fetch updates for an existing repository at `repo_path`.
///
/// TODO(native-ssh): Migrate to gix native SSH when stable.
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
///
/// TODO(native-ssh): Replace with gix worktree API when it supports
/// efficient working tree updates after fetch.
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
    let mut prepare = gix::prepare_clone(url, target_path).context("failed to prepare clone")?;

    if let Some(ref branch) = opts.branch {
        prepare = prepare
            .with_ref_name(Some(branch.as_str()))
            .context("invalid branch name")?;
    }

    if let Some(d) = opts.depth {
        prepare = prepare.with_shallow(shallow_depth(d));
    }

    // TODO(native-ssh): Once gix supports native SSH transport natively,
    // configure the connection here instead of relying on the system ssh command.

    let (mut checkout, _outcome) = prepare
        .fetch_then_checkout(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)
        .context("clone fetch failed")?;

    checkout
        .main_worktree(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)
        .context("checkout failed")?;

    Ok(())
}

fn fetch_blocking(repo_path: &Path, opts: &FetchOptions) -> Result<()> {
    let repo = gix::open(repo_path).context("failed to open repository")?;

    let remote = repo
        .find_remote("origin")
        .context("remote 'origin' not found")?;

    let connection = remote
        .connect(gix::remote::Direction::Fetch)
        .context("failed to connect to remote")?;

    let mut fetch_cmd = connection
        .prepare_fetch(gix::progress::Discard, Default::default())
        .context("failed to prepare fetch")?;

    if let Some(d) = opts.depth {
        fetch_cmd = fetch_cmd.with_shallow(shallow_depth(d));
    }

    let _outcome = fetch_cmd
        .receive(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)
        .context("fetch failed")?;

    Ok(())
}

fn reset_to_remote_blocking(repo_path: &Path, branch: Option<&str>) -> Result<()> {
    let target = match branch {
        Some(b) => format!("origin/{b}"),
        None => "@{upstream}".to_string(),
    };

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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn shallow_depth(depth: u32) -> gix::remote::fetch::Shallow {
    gix::remote::fetch::Shallow::DepthAtRemote(
        std::num::NonZero::new(depth).expect("depth must be > 0"),
    )
}

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
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("failed to open repository"),
            "unexpected error: {msg}"
        );
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
