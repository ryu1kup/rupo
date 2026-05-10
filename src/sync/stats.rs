//! Sync statistics for priority scheduling.
//!
//! Records per-project sync duration so that future runs can prioritize
//! large (slow) projects first, maximizing parallelism utilization.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const STATS_FILE: &str = "sync-stats.toml";

/// Aggregated sync statistics for all projects.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SyncStats {
    /// Keyed by project path (e.g. `"mcu"`, `"app/core"`).
    #[serde(default)]
    pub projects: HashMap<String, ProjectStats>,
}

/// Recorded statistics for a single project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectStats {
    /// Wall-clock duration of the last successful sync in milliseconds.
    pub duration_ms: u64,
}

impl SyncStats {
    /// Load stats from `.rupo/sync-stats.toml`. Returns empty stats if the
    /// file does not exist.
    pub fn load(workspace: &Path) -> Result<Self> {
        let path = workspace.join(STATS_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let stats: Self = toml::from_str(&content)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        Ok(stats)
    }

    /// Save stats to `.rupo/sync-stats.toml`. Merges with any existing data
    /// so that projects not synced in this run retain their previous stats.
    pub fn save(&self, workspace: &Path) -> Result<()> {
        let path = workspace.join(STATS_FILE);
        let content = toml::to_string_pretty(self).context("failed to serialize sync stats")?;
        std::fs::write(&path, content)
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }

    /// Merge new duration measurements into existing stats.
    pub fn merge(&mut self, results: HashMap<String, u64>) {
        for (path, duration_ms) in results {
            self.projects.insert(path, ProjectStats { duration_ms });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_nonexistent_returns_empty() {
        let stats = SyncStats::load(Path::new("/tmp/no-such-dir-rupo-test")).unwrap();
        assert!(stats.projects.is_empty());
    }

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut stats = SyncStats::default();
        stats.merge(HashMap::from([
            ("mcu".to_string(), 92000),
            ("app".to_string(), 3200),
        ]));

        stats.save(dir.path()).unwrap();
        let loaded = SyncStats::load(dir.path()).unwrap();

        assert_eq!(loaded.projects.len(), 2);
        assert_eq!(loaded.projects["mcu"].duration_ms, 92000);
        assert_eq!(loaded.projects["app"].duration_ms, 3200);
    }

    #[test]
    fn merge_overwrites_existing() {
        let mut stats = SyncStats::default();
        stats.merge(HashMap::from([("mcu".to_string(), 90000)]));
        stats.merge(HashMap::from([("mcu".to_string(), 85000)]));
        assert_eq!(stats.projects["mcu"].duration_ms, 85000);
    }
}
