//! Stage management operations
//!
//! This module handles stage-related operations including fetching, listing,
//! updating, and installing packages to stages.

use crate::crossdev::CrossdevError;
use crossdev_config::PlatformConfig;
use crossdev_sandbox::{auto_detect_backend, SandboxError};
use etcetera::{base_strategy::Xdg, BaseStrategy};
use jiff::Timestamp;
use log::info;
use serde::{Deserialize, Serialize};
use serde_json;
use std::path::{Path, PathBuf};
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
            )
            .await?;

        // Step 2: Update binutils-libs
        info!("Updating binutils-libs...");
        backend
            .run_command(
                "default",
                &format!("{}-emerge", target_chost),
                &["-b", "-k", "sys-libs/binutils-libs"],
            )
            .await?;

        // Step 3: Update system packages
        info!("Updating system packages...");
        backend
            .run_command(
                "default",
                &format!("{}-emerge", target_chost),
                &["-b", "-k", "-u", "system"],
            )
            .await?;

        // Step 4: Update world packages in the stage directory
        info!("Updating world packages in stage...");

        backend
            .run_command(
                "default",
                &format!("{}-emerge", target_chost),
                &["-k", "-e", "@world"],
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
            )
            .await?;

        info!("Ldconfig update completed: {}", ldconfig_result);

        Ok(())
    }
}

/// Sandbox state tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxState {
    /// Sandbox name
    pub name: String,

    /// Current state of the sandbox
    pub state: SandboxStatus,

    /// Optional stage loaded in this sandbox
    pub loaded_stage: Option<String>,

    /// Timestamp of last update
    pub last_updated: String,

    /// Sandbox creation timestamp
    pub created_at: String,
}

/// Possible sandbox states
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SandboxStatus {
    #[serde(rename = "new")]
    New,

    #[serde(rename = "prepared")]
    Prepared,

    #[serde(rename = "stage_loaded")]
    StageLoaded,

    #[serde(rename = "updating")]
    Updating,

    #[serde(rename = "error")]
    Error,
}

/// Sandbox registry for tracking all sandboxes and their states
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SandboxRegistry {
    sandboxes: Vec<SandboxState>,
}

impl SandboxRegistry {
    /// Get the default registry path
    pub fn get_default_registry_path() -> PathBuf {
        let mut state_dir = Xdg::new().unwrap().state_dir().unwrap_or_else(|| {
            let mut path = PathBuf::from(".local");
            path.push("state");
            path
        });
        state_dir.push("crossdev-stages");
        state_dir.push("sandboxes.json");
        state_dir
    }

    /// Load registry from file
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, StageError> {
        let path = path.as_ref();

        // Create file if it doesn't exist
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path).map_err(|e| StageError::IoError(e))?;

        serde_json::from_str(&content).map_err(|e| {
            StageError::ConfigError(format!("Failed to parse sandbox registry: {}", e))
        })
    }

    /// Save registry to file
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), StageError> {
        // Create parent directory if it doesn't exist
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| StageError::IoError(e))?;
        }

        let content = serde_json::to_string_pretty(self).map_err(|e| {
            StageError::ConfigError(format!("Failed to serialize sandbox registry: {}", e))
        })?;

        std::fs::write(path, content).map_err(|e| StageError::IoError(e))?;

        Ok(())
    }

    /// Add or update a sandbox in the registry
    pub fn upsert_sandbox(&mut self, sandbox: SandboxState) -> Result<(), StageError> {
        // Remove existing sandbox with same name
        self.sandboxes.retain(|s| s.name != sandbox.name);

        // Add the new/updated sandbox
        self.sandboxes.push(sandbox);

        Ok(())
    }

    /// Get a sandbox by name
    pub fn get_sandbox(&self, name: &str) -> Option<&SandboxState> {
        self.sandboxes.iter().find(|s| s.name == name)
    }

    /// List all sandboxes
    pub fn list_sandboxes(&self) -> &[SandboxState] {
        &self.sandboxes
    }

    /// Remove a sandbox from the registry
    pub fn remove_sandbox(&mut self, name: &str) -> Result<SandboxState, StageError> {
        let index = self
            .sandboxes
            .iter()
            .position(|s| s.name == name)
            .ok_or_else(|| StageError::ConfigError(format!("Sandbox '{}' not found", name)))?;

        Ok(self.sandboxes.remove(index))
    }

    /// Create a new sandbox state
    pub fn create_sandbox_state(name: &str, status: SandboxStatus) -> SandboxState {
        let now = Timestamp::now();
        let timestamp_str = now.strftime("%Y%m%dT%H").to_string();

        SandboxState {
            name: name.to_string(),
            state: status,
            loaded_stage: None,
            last_updated: timestamp_str.clone(),
            created_at: timestamp_str,
        }
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
