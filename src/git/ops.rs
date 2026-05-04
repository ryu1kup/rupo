//! Git operations abstraction layer.
//!
//! All Git I/O goes through this module so that the rest of the codebase
//! (cli/, sync/, etc.) never touches `gix` directly.
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
// Public API
// ---------------------------------------------------------------------------

/// Clone a repository from `url` into `target_path`.
///
/// SSH URLs (e.g. `git@github.com:org/repo.git`) are supported through
/// the system `ssh` command.
///
/// TODO(native-ssh): Migrate to gix native SSH when stable.
pub fn clone(url: &str, target_path: &Path, opts: &CloneOptions) -> Result<()> {
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

    let (_repo, _outcome) = prepare
        .fetch_only(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)
        .context("clone fetch failed")?;

    Ok(())
}

/// Fetch updates for an existing repository at `repo_path`.
///
/// TODO(native-ssh): Migrate to gix native SSH when stable.
pub fn fetch(repo_path: &Path, opts: &FetchOptions) -> Result<()> {
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
    use std::path::PathBuf;

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
    fn clone_with_invalid_url_returns_error() {
        let target = std::env::temp_dir().join("rupo-test-nonexistent");
        let result = clone("://not-a-url", &target, &CloneOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn fetch_with_nonexistent_repo_returns_error() {
        let path = std::env::temp_dir().join("rupo-test-no-such-repo");
        let result = fetch(&path, &FetchOptions::default());
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
}
