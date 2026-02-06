//! Stage management operations
//!
//! This module handles stage-related operations including fetching, listing,
//! updating, and installing packages to stages.

use crate::crossdev::{CrossdevEnvironment, CrossdevError};
use crossdev_config::PlatformConfig;
use crossdev_sandbox::{auto_detect_backend, SandboxError};
use crossdev_stage3::{Stage3Fetcher, Stage3Info};
use log::info;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Stage management errors
#[derive(Debug, Error)]
pub enum StageError {
    #[error("Stage3 error: {0}")]
    Stage3Error(#[from] crossdev_stage3::Stage3Error),

    #[error("Config error: {0}")]
    ConfigError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Sandbox error: {0}")]
    SandboxError(#[from] SandboxError),

    #[error("Crossdev error: {0}")]
    CrossdevError(#[from] CrossdevError),

    #[error("Package installation error: {0}")]
    PackageError(String),

    #[error("Registry error: {0}")]
    RegistryError(String),
}

/// Information about an extracted stage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedStage {
    /// Unique name/identifier for the stage
    pub name: String,

    /// Path to the stage directory
    pub path: PathBuf,

    /// Target architecture
    pub target_arch: String,

    /// Target flavor
    pub target_flavor: String,

    /// Base stage3 image name
    pub base_image: String,

    /// Creation timestamp
    pub created_at: String,

    /// Last updated timestamp
    pub last_updated: String,

    /// Status (ready, updating, etc.)
    pub status: String,
}

/// Stage registry for tracking extracted stages
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct StageRegistry {
    stages: Vec<ExtractedStage>,
}

impl StageRegistry {
    /// Get the default registry path
    pub fn get_default_registry_path() -> PathBuf {
        let mut config_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from(".config"));
        config_dir.push("crossdev-stages");
        config_dir.push("stages.toml");
        config_dir
    }

    /// Get the default stages directory
    pub fn get_default_stages_dir() -> PathBuf {
        let mut data_dir = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".local/share"));
        data_dir.push("crossdev-stages");
        data_dir.push("stages");
        data_dir
    }

    /// Load registry from file
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, StageError> {
        let path = path.as_ref();

        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| StageError::IoError(e))?;
        }

        // Check if file exists
        if !path.exists() {
            return Ok(Self::default()); // Return empty registry if file doesn't exist
        }

        // Read file content
        let content =
            std::fs::read_to_string(path).map_err(|e| StageError::RegistryError(e.to_string()))?;

        // Parse TOML
        let registry: StageRegistry =
            toml::from_str(&content).map_err(|e| StageError::RegistryError(e.to_string()))?;

        Ok(registry)
    }

    /// Save registry to file
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), StageError> {
        // Create parent directory if it doesn't exist
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| StageError::IoError(e))?;
        }

        let content =
            toml::to_string(self).map_err(|e| StageError::RegistryError(e.to_string()))?;

        std::fs::write(path, content).map_err(|e| StageError::RegistryError(e.to_string()))?;

        Ok(())
    }

    /// Add a stage to the registry
    pub fn add_stage(&mut self, stage: ExtractedStage) -> Result<(), StageError> {
        // Check if stage with same name already exists
        if self.stages.iter().any(|s| s.name == stage.name) {
            return Err(StageError::RegistryError(format!(
                "Stage '{}' already exists",
                stage.name
            )));
        }

        self.stages.push(stage);
        Ok(())
    }

    /// Get a stage by name
    pub fn get_stage(&self, name: &str) -> Option<&ExtractedStage> {
        self.stages.iter().find(|s| s.name == name)
    }

    /// List all stages
    pub fn list_stages(&self) -> &[ExtractedStage] {
        &self.stages
    }

    /// Remove a stage from the registry
    pub fn remove_stage(&mut self, name: &str) -> Result<ExtractedStage, StageError> {
        let pos = self
            .stages
            .iter()
            .position(|s| s.name == name)
            .ok_or_else(|| StageError::RegistryError(format!("Stage '{}' not found", name)))?;

        Ok(self.stages.remove(pos))
    }

    /// Generate a unique stage name
    pub fn generate_stage_name(&self, base_name: &str) -> String {
        let mut name = base_name.to_string();
        let mut counter = 1;

        while self.stages.iter().any(|s| s.name == name) {
            name = format!("{}-{}", base_name, counter);
            counter += 1;
        }

        name
    }
}

/// Stage manager
pub struct StageManager {
    config: PlatformConfig,
    cache_dir: PathBuf,
    mirror_url: String,
}

impl StageManager {
    /// Create a new StageManager
    pub fn new(config: PlatformConfig, cache_dir: impl AsRef<Path>, mirror_url: &str) -> Self {
        Self {
            config,
            cache_dir: cache_dir.as_ref().to_path_buf(),
            mirror_url: mirror_url.to_string(),
        }
    }

    /// Fetch latest stage3 image
    pub fn fetch_latest(&self) -> Result<Stage3Info, StageError> {
        let fetcher = Stage3Fetcher::new_from_platform_config(self.config.clone(), &self.cache_dir, &self.mirror_url);
        fetcher.fetch_latest().map_err(StageError::Stage3Error)
    }

    /// List available stage3 flavors
    pub fn list_available_flavors(&self) -> Result<Vec<String>, StageError> {
        let fetcher = Stage3Fetcher::new_from_platform_config(self.config.clone(), &self.cache_dir, &self.mirror_url);
        fetcher
            .list_available_flavors()
            .map_err(StageError::Stage3Error)
    }

    /// List cached stage3 images
    pub fn list_cached_images(&self) -> Result<Vec<Stage3Info>, StageError> {
        let fetcher = Stage3Fetcher::new_from_platform_config(self.config.clone(), &self.cache_dir, &self.mirror_url);
        fetcher.get_cached_images().map_err(StageError::Stage3Error)
    }

    /// Extract stage3 to target directory
    pub fn extract_stage3(
        &self,
        stage3: &Stage3Info,
        target_dir: impl AsRef<Path>,
    ) -> Result<(), StageError> {
        let fetcher = Stage3Fetcher::new_from_platform_config(self.config.clone(), &self.cache_dir, &self.mirror_url);
        fetcher
            .extract_stage3(stage3, target_dir)
            .map_err(StageError::Stage3Error)
    }

    /// Update a stage3 by installing system updates
    pub async fn update_stage3(&self, stage_dir: impl AsRef<Path>) -> Result<(), StageError> {
        let stage_dir = stage_dir.as_ref();
        info!("Updating stage3 at: {}", stage_dir.display());

        let backend = auto_detect_backend()?;
        let target_chost = self.config.compilation.chost.clone();

        // Update system packages
        let update_result = backend
            .run_command(
                "default",
                &format!("{}-emerge", target_chost),
                &["-b", "-k", "-u", "system"],
                Some(stage_dir),
            )
            .await?;

        info!("Stage3 update completed: {}", update_result);

        Ok(())
    }

    /// Install additional packages to a stage
    pub async fn install_packages(
        &self,
        stage_dir: impl AsRef<Path>,
        packages: &[&str],
    ) -> Result<(), StageError> {
        let stage_dir = stage_dir.as_ref();
        info!("Installing packages to stage at: {}", stage_dir.display());

        let backend = auto_detect_backend()?;
        let target_chost = self.config.compilation.chost.clone();

        // Convert packages to command arguments
        let mut emerge_args = vec!["-b", "-k"];
        for package in packages {
            emerge_args.push(package);
        }

        let install_result = backend
            .run_command(
                "default",
                &format!("{}-emerge", target_chost),
                &emerge_args,
                Some(stage_dir),
            )
            .await?;

        info!("Package installation completed: {}", install_result);

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

    /// Prepare crossdev environment for the target
    pub async fn prepare_crossdev(&self) -> Result<(), StageError> {
        info!(
            "Preparing crossdev environment for {}",
            self.config.compilation.chost
        );

        let backend = auto_detect_backend()?;
        let crossdev_root = format!("/usr/{}", self.config.compilation.chost);

        let crossdev_env = CrossdevEnvironment::new(
            &self.config.compilation.chost,
            &crossdev_root,
            &self.config.compilation.profile,
        );

        crossdev_env.initialize(&*backend).await?;

        info!("Crossdev environment prepared successfully");

        Ok(())
    }

    /// Extract stage3 with registry tracking
    pub fn extract_stage3_with_registry(
        &self,
        stage3: &Stage3Info,
        stage_name: Option<&str>,
    ) -> Result<ExtractedStage, StageError> {
        // Load or create stage registry
        let registry_path = StageRegistry::get_default_registry_path();
        let mut registry = StageRegistry::load_from_file(&registry_path)?;

        // Determine stage name
        let base_name = format!("{}-{}", stage3.arch, stage3.flavor);
        let stage_name = if let Some(name) = stage_name {
            name.to_string()
        } else {
            registry.generate_stage_name(&base_name)
        };

        // Create stage directory
        let stages_dir = StageRegistry::get_default_stages_dir();
        let stage_dir = stages_dir.join(&stage_name);

        // Create parent directory if it doesn't exist
        std::fs::create_dir_all(&stages_dir).map_err(|e| StageError::IoError(e))?;

        info!("Extracting stage3 to: {}", stage_dir.display());

        // Extract the stage3
        let fetcher = Stage3Fetcher::new_from_platform_config(self.config.clone(), &self.cache_dir, &self.mirror_url);
        fetcher.extract_stage3(stage3, &stage_dir)?;

        // Create registry entry
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let extracted_stage = ExtractedStage {
            name: stage_name.clone(),
            path: stage_dir.clone(),
            target_arch: stage3.arch.clone(),
            target_flavor: stage3.flavor.clone(),
            base_image: stage3.name.clone(),
            created_at: now.clone(),
            last_updated: now,
            status: "ready".to_string(),
        };

        // Add to registry
        registry.add_stage(extracted_stage.clone())?;
        registry.save_to_file(&registry_path)?;

        info!(
            "Stage '{}' extracted and registered successfully",
            stage_name
        );
        info!("  Location: {}", stage_dir.display());
        info!("  Arch: {}", stage3.arch);
        info!("  Flavor: {}", stage3.flavor);

        Ok(extracted_stage)
    }

    /// List all registered stages
    pub fn list_registered_stages() -> Result<Vec<ExtractedStage>, StageError> {
        let registry_path = StageRegistry::get_default_registry_path();
        let registry = StageRegistry::load_from_file(&registry_path)?;

        Ok(registry.list_stages().to_vec())
    }

    /// Get stage by name
    pub fn get_registered_stage(name: &str) -> Result<ExtractedStage, StageError> {
        let registry_path = StageRegistry::get_default_registry_path();
        let registry = StageRegistry::load_from_file(&registry_path)?;

        registry
            .get_stage(name)
            .cloned()
            .ok_or_else(|| StageError::RegistryError(format!("Stage '{}' not found", name)))
    }

    /// Remove a registered stage
    pub fn remove_registered_stage(name: &str) -> Result<(), StageError> {
        let registry_path = StageRegistry::get_default_registry_path();
        let mut registry = StageRegistry::load_from_file(&registry_path)?;

        let stage = registry.remove_stage(name)?;

        // Remove the stage directory if it exists
        if stage.path.exists() {
            std::fs::remove_dir_all(&stage.path).map_err(|e| StageError::IoError(e))?;
        }

        // Save updated registry
        registry.save_to_file(&registry_path)?;

        info!("Stage '{}' removed successfully", name);
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
