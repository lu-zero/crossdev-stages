//! Stage management operations
//!
//! This module handles stage-related operations including fetching, listing,
//! updating, and installing packages to stages.

use crate::crossdev::CrossdevError;
use crossdev_config::PlatformConfig;
use crossdev_sandbox::{auto_detect_backend, SandboxError};
use log::info;
use std::path::Path;
use thiserror::Error;

/// Stage management errors
#[derive(Debug, Error)]
pub enum StageError {
    #[error("Stage3 error: {0}")]
    Stage3Error(#[from] crossdev_stages::Stage3Error),

    #[error("Config error: {0}")]
    ConfigError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Sandbox error: {0}")]
    SandboxError(#[from] SandboxError),

    #[error("Crossdev error: {0}")]
    CrossdevError(#[from] CrossdevError),
}

/// Stage manager
pub struct StageManager {
    config: PlatformConfig,
}

impl StageManager {
    /// Create a new StageManager
    pub fn new(config: PlatformConfig, _cache_dir: impl AsRef<Path>, _mirror_url: &str) -> Self {
        Self { config }
    }

    /// Update a stage3 by installing system updates
    pub async fn update_stage3(&self, stage_dir: impl AsRef<Path>) -> Result<(), StageError> {
        let stage_dir = stage_dir.as_ref();
        info!("Updating stage3 at: {}", stage_dir.display());

        let backend = auto_detect_backend()?;
        let target_chost = self.config.compilation.chost.clone();

        // Step 1: Update gcc
        info!("Updating gcc...");
        backend
            .run_command(
                "default",
                &format!("{}-emerge", target_chost),
                &["-b", "-k", "gcc"],
                None, // Run in host context
            )
            .await?;

        // Step 2: Update binutils-libs
        info!("Updating binutils-libs...");
        backend
            .run_command(
                "default",
                &format!("{}-emerge", target_chost),
                &["-b", "-k", "sys-libs/binutils-libs"],
                None, // Run in host context
            )
            .await?;

        // Step 3: Update system packages
        info!("Updating system packages...");
        backend
            .run_command(
                "default",
                &format!("{}-emerge", target_chost),
                &["-b", "-k", "-u", "system"],
                None, // Run in host context
            )
            .await?;

        // Step 4: Update world packages in the stage directory
        info!("Updating world packages in stage...");
        backend
            .run_command(
                "default",
                &format!("{}-emerge", target_chost),
                &["-k", "-e", "@world"],
                Some(stage_dir), // Run in stage context
            )
            .await?;

        info!("Stage3 update completed successfully");

        Ok(())
    }

    /// Update ldconfig cache for a stage
    pub async fn update_ldconfig(&self, stage_dir: impl AsRef<Path>) -> Result<(), StageError> {
        let stage_dir = stage_dir.as_ref();
        info!(
            "Updating ldconfig cache for stage at: {}",
            stage_dir.display()
        );

        let backend = auto_detect_backend()?;

        let ldconfig_result = backend
            .run_command(
                "default",
                "ldconfig",
                &["-v", "-r", stage_dir.to_str().unwrap()],
                None,
            )
            .await?;

        info!("Ldconfig update completed: {}", ldconfig_result);

        Ok(())
    }
}

/// Load platform configuration
pub fn load_platform_config(platform: &str) -> Result<PlatformConfig, StageError> {
    let config_file = format!("config/platforms/{}.toml", platform);
    PlatformConfig::load_from_file(&config_file).map_err(|e| StageError::ConfigError(e.to_string()))
}

/// Get default platform configuration
pub fn get_default_platform_config() -> Result<PlatformConfig, StageError> {
    load_platform_config("riscv64-k1")
}
