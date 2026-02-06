use thiserror::Error;

/// Combined error type for crossdev-stages operations
#[derive(Debug, Error)]
pub enum StageError {
    /// Cache-related errors
    #[error("Cache error: {0}")]
    CacheError(#[from] super::cache::CacheError),

    /// Stage3-related errors  
    #[error("Stage3 error: {0}")]
    Stage3Error(#[from] super::stage3::Stage3Error),

    /// Configuration errors
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// IO errors
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}
