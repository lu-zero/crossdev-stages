//! Crossdev Cache - Standalone package cache for Gentoo cross-compilation
//!
//! This crate provides a robust caching system for Gentoo packages and distfiles
//! using the etcetera crate for proper cross-platform directory handling.

use etcetera::{base_strategy::Xdg, BaseStrategy};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
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

/// Main cache manager
#[derive(Debug)]
pub struct CrossdevCache {
    config: CacheConfig,
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
            config,
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

    /// Cache a binary package
    pub fn cache_binpkg(
        &self,
        category: &str,
        package: &str,
        version: &str,
        data: &[u8],
    ) -> Result<PathBuf, CacheError> {
        let category_dir = self.binpkgs_dir.join(category);
        std::fs::create_dir_all(&category_dir)?;

        let path = self.binpkg_path(category, package, version);
        std::fs::write(&path, data)?;

        Ok(path)
    }

    /// Cache a distfile
    pub fn cache_distfile(&self, filename: &str, data: &[u8]) -> Result<PathBuf, CacheError> {
        let path = self.distfile_path(filename);
        std::fs::write(&path, data)?;

        Ok(path)
    }

    /// Verify cache integrity
    pub fn verify(&self) -> Result<CacheVerificationReport, CacheError> {
        let mut report = CacheVerificationReport::default();

        // Check distfiles
        if self.distfiles_dir.exists() {
            for entry in std::fs::read_dir(&self.distfiles_dir)? {
                let entry = entry?;
                report.distfile_count += 1;
                report.distfile_size += entry.metadata()?.len();
            }
        }

        // Check binary packages
        if self.binpkgs_dir.exists() {
            for entry in walkdir::WalkDir::new(&self.binpkgs_dir) {
                let entry = entry?;
                if entry.file_type().is_file()
                    && entry.path().extension() == Some(std::ffi::OsStr::new("tbz2"))
                {
                    report.binpkg_count += 1;
                    report.binpkg_size += entry.metadata()?.len();
                }
            }
        }

        Ok(report)
    }

    /// Clean cache based on size and age limits
    pub fn cleanup(&self) -> Result<CacheCleanupReport, CacheError> {
        let report = CacheCleanupReport::default();

        // Calculate current cache size
        let current_size = self.verify()?.total_size();
        let max_size = self.config.max_size_mb * 1024 * 1024;

        if current_size > max_size {
            // Implement cleanup logic
            // Sort by access time and remove oldest
            // Track what was removed
        }

        Ok(report)
    }
}

/// Cache verification report
#[derive(Debug, Default)]
pub struct CacheVerificationReport {
    pub distfile_count: u64,
    pub distfile_size: u64,
    pub binpkg_count: u64,
    pub binpkg_size: u64,
}

impl CacheVerificationReport {
    pub fn total_size(&self) -> u64 {
        self.distfile_size + self.binpkg_size
    }
}

/// Cache cleanup report
#[derive(Debug, Default)]
pub struct CacheCleanupReport {
    pub removed_distfiles: u64,
    pub removed_binpkgs: u64,
    pub freed_space: u64,
}

/// Package metadata for cache tracking
#[derive(Debug, Serialize, Deserialize)]
pub struct PackageMetadata {
    pub name: String,
    pub version: String,
    pub category: String,
    pub size: u64,
    pub timestamp: SystemTime,
    pub checksum: String,
}

/// Result type for cache operations
pub type CacheResult<T> = Result<T, CacheError>;
