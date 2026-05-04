use thiserror::Error;

/// Errors that can occur during rupo operations.
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum RupoError {
    #[error("manifest parse error: {0}")]
    ManifestParse(String),

    #[error("workspace already initialized at {0}")]
    AlreadyInitialized(std::path::PathBuf),

    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}
