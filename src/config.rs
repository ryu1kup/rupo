use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const CONFIG_FILE: &str = "config.toml";

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub url: String,
    pub branch: Option<String>,
    pub manifest: String,
    pub mirror: bool,
}

impl Config {
    /// Load configuration from `<workspace>/config.toml`.
    pub fn load(workspace: &Path) -> Result<Self> {
        let path = workspace.join(CONFIG_FILE);
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))
    }

    /// Save configuration to `<workspace>/config.toml`.
    pub fn save(&self, workspace: &Path) -> Result<()> {
        let path = workspace.join(CONFIG_FILE);
        let content = toml::to_string_pretty(self).context("failed to serialize config.toml")?;
        std::fs::write(&path, content)
            .with_context(|| format!("failed to write {}", path.display()))
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            url: String::new(),
            branch: None,
            manifest: "default.xml".to_string(),
            mirror: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn save_and_load_roundtrip_with_all_fields() {
        let tmp = TempDir::new().unwrap();
        let config = Config {
            url: "git@github.com:org/manifests.git".to_string(),
            branch: Some("main".to_string()),
            manifest: "custom.xml".to_string(),
            mirror: true,
        };

        config.save(tmp.path()).unwrap();
        let loaded = Config::load(tmp.path()).unwrap();

        assert_eq!(loaded.url, "git@github.com:org/manifests.git");
        assert_eq!(loaded.branch.as_deref(), Some("main"));
        assert_eq!(loaded.manifest, "custom.xml");
        assert!(loaded.mirror);
    }

    #[test]
    fn save_and_load_roundtrip_with_optional_none() {
        let tmp = TempDir::new().unwrap();
        let config = Config {
            url: "https://example.com/repo.git".to_string(),
            branch: None,
            manifest: "default.xml".to_string(),
            mirror: false,
        };

        config.save(tmp.path()).unwrap();
        let loaded = Config::load(tmp.path()).unwrap();

        assert_eq!(loaded.url, "https://example.com/repo.git");
        assert!(loaded.branch.is_none());
        assert_eq!(loaded.manifest, "default.xml");
        assert!(!loaded.mirror);
    }

    #[test]
    fn load_from_nonexistent_path_returns_error() {
        let tmp = TempDir::new().unwrap();
        let result = Config::load(tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn load_from_invalid_toml_returns_error() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(CONFIG_FILE), "not valid {{toml").unwrap();
        let result = Config::load(tmp.path());
        assert!(result.is_err());
    }
}
