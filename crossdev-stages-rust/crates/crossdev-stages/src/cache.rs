//! Cache functionality - Migrated from crossdev-cache
//!
//! Provides XDG-compliant caching system for Gentoo packages and distfiles

use etcetera::{base_strategy::Xdg, BaseStrategy};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;
use walkdir::Error as WalkDirError;

/// Cache errors
#[derive(Error, Debug)]
pub enum CacheError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Cache directory setup failed: {0}")]
    SetupFailed(String),

    #[error("Package verification failed: {0}")]
    VerificationFailed(String),

    #[error("Cache configuration error: {0}")]
    ConfigError(String),

    #[error("WalkDir error: {0}")]
    WalkDirError(#[from] WalkDirError),
}

/// Cache directory strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CacheStrategy {
    /// Use .local/crossdev-stages in user's home directory
    Local,
    /// Use system-wide cache directory
    System,
    /// Use custom path
    Custom(String),
}

/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Enable caching
    pub enabled: bool,

    /// Cache directory strategy
    pub strategy: CacheStrategy,

    /// Maximum cache size in MB
    pub max_size_mb: u64,

    /// Maximum package age in days
    pub max_age_days: u64,

    /// Verify packages on startup
    pub verify_on_start: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strategy: CacheStrategy::Local,
            max_size_mb: 10_240, // 10GB
            max_age_days: 30,
            verify_on_start: true,
        }
    }
}

/// Main cache manager
#[derive(Debug)]
pub struct CrossdevCache {
    _config: CacheConfig,
    cache_dir: PathBuf,
    distfiles_dir: PathBuf,
    binpkgs_dir: PathBuf,
}

impl CrossdevCache {
    /// Create new cache instance
    pub fn new(config: CacheConfig) -> Result<Self, CacheError> {
        // Determine cache directory based on strategy
        let cache_dir = match config.strategy {
            CacheStrategy::Local => {
                // Use .local/crossdev-stages as requested
                let xdg = match Xdg::new() {
                    Ok(xdg) => xdg,
                    Err(e) => {
                        return Err(CacheError::ConfigError(format!(
                            "Failed to get XDG directories: {}",
                            e
                        )))
                    }
                };
                let local_dir = xdg.home_dir().join(".local/crossdev-stages");
                std::fs::create_dir_all(&local_dir)?;
                local_dir
            }
            CacheStrategy::System => {
                let xdg = match Xdg::new() {
                    Ok(xdg) => xdg,
                    Err(e) => {
                        return Err(CacheError::ConfigError(format!(
                            "Failed to get XDG directories: {}",
                            e
                        )))
                    }
                };
                xdg.cache_dir().join("crossdev-stages")
            }
            CacheStrategy::Custom(ref path) => PathBuf::from(path),
        };

        // Create subdirectories
        let distfiles_dir = cache_dir.join("distfiles");
        let binpkgs_dir = cache_dir.join("binpkgs");

        std::fs::create_dir_all(&distfiles_dir)?;
        std::fs::create_dir_all(&binpkgs_dir)?;

        Ok(Self {
            _config: config,
            cache_dir,
            distfiles_dir,
            binpkgs_dir,
        })
    }

    /// Get cache directory
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Get distfiles directory
    pub fn distfiles_dir(&self) -> &Path {
        &self.distfiles_dir
    }

    /// Get binary packages directory
    pub fn binpkgs_dir(&self) -> &Path {
        &self.binpkgs_dir
    }

    /// Get path for a binary package (Gentoo .tbz2 format)
    pub fn binpkg_path(&self, category: &str, package: &str, version: &str) -> PathBuf {
        self.binpkgs_dir
            .join(category)
            .join(format!("{}-{}.tbz2", package, version))
    }

    /// Get path for a distfile
    pub fn distfile_path(&self, filename: &str) -> PathBuf {
        self.distfiles_dir.join(filename)
    }

    /// Check if binary package exists in cache
    pub fn has_binpkg(&self, category: &str, package: &str, version: &str) -> bool {
        let path = self.binpkg_path(category, package, version);
        path.exists()
    }

    /// Check if distfile exists in cache
    pub fn has_distfile(&self, filename: &str) -> bool {
        let path = self.distfile_path(filename);
        path.exists()
    }
}
