//! Configuration management for crossdev-stages
//!
//! This crate provides functionality for loading and managing
//! platform-specific configuration files.

use serde::Deserialize;
use std::path::Path;
use thiserror::Error;

/// Configuration error types
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Configuration file not found: {0}")]
    NotFound(String),

    #[error("Configuration file parse error: {0}")]
    ParseError(String),

    #[error("Configuration validation error: {0}")]
    ValidationError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Target architecture configuration
#[derive(Debug, Deserialize, Clone)]
pub struct TargetConfig {
    pub arch: String,
    pub chost: String,
    pub flavor: String,
    pub keyword: String,
}

/// Compilation settings configuration
#[derive(Debug, Deserialize, Clone)]
pub struct CompilationConfig {
    pub cflags: String,
    pub gcc_version: String,
    pub profile: String,
    #[serde(default = "default_makeopts")]
    pub makeopts: String,
    #[serde(default = "default_emerge_opts")]
    pub emerge_default_opts: String,
}

fn default_makeopts() -> String {
    "-j$(nproc) --load-average=$(nproc)".to_string()
}

fn default_emerge_opts() -> String {
    "--jobs=$(nproc) --load-average=$(nproc) --quiet-build y".to_string()
}

/// Repository configuration
#[derive(Debug, Deserialize, Clone)]
pub struct RepositoryConfig {
    pub opensbi_repo: String,
    pub opensbi_tag: String,
    pub u_boot_repo: String,
    pub u_boot_tag: String,
    pub firmware_repo: String,
    pub firmware_tag: String,
    pub kernel_repo: String,
    pub kernel_tag: String,
    pub bootloader_tag: String,
}

/// Package configuration
#[derive(Debug, Deserialize, Clone)]
pub struct PackageConfig {
    pub stage1_file: String,
    pub additional_file: String,
}

/// Image configuration
#[derive(Debug, Deserialize, Clone)]
pub struct ImageConfig {
    pub root_size: String,
    pub boot_size: String,
    pub genimage_config: String,
}

/// Main platform configuration structure
#[derive(Debug, Deserialize, Clone)]
pub struct PlatformConfig {
    pub target: TargetConfig,
    pub compilation: CompilationConfig,
    pub repositories: RepositoryConfig,
    pub packages: PackageConfig,
    pub image: ImageConfig,
}

impl PlatformConfig {
    /// Load configuration from a TOML file
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let path = path.as_ref();

        // Check if file exists
        if !path.exists() {
            return Err(ConfigError::NotFound(path.to_string_lossy().into_owned()));
        }

        // Read file content
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::IoError(e))?;

        // Parse TOML
        let config: PlatformConfig =
            toml::from_str(&content).map_err(|e| ConfigError::ParseError(e.to_string()))?;

        // Validate configuration
        config.validate()?;

        Ok(config)
    }

    /// Validate configuration
    fn validate(&self) -> Result<(), ConfigError> {
        // Check required fields are not empty
        if self.target.arch.is_empty() {
            return Err(ConfigError::ValidationError(
                "target.arch cannot be empty".to_string(),
            ));
        }

        if self.target.chost.is_empty() {
            return Err(ConfigError::ValidationError(
                "target.chost cannot be empty".to_string(),
            ));
        }

        // Add more validation as needed

        Ok(())
    }

    /// Get the cross-compile prefix (e.g., "riscv64-unknown-linux-gnu-")
    pub fn cross_compile_prefix(&self) -> String {
        format!("{}-", self.target.chost)
    }
}

/// Load package list from file
pub fn load_package_list<P: AsRef<Path>>(path: P) -> Result<Vec<String>, ConfigError> {
    let path = path.as_ref();

    if !path.exists() {
        return Err(ConfigError::NotFound(path.to_string_lossy().into_owned()));
    }

    let content = std::fs::read_to_string(path).map_err(|e| ConfigError::IoError(e))?;

    let mut packages = Vec::new();

    for line in content.lines() {
        // Skip comments and empty lines
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        packages.push(line.to_string());
    }

    Ok(packages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_load_valid_config() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("test.toml");

        let config_content = r#"
            [target]
            arch = "riscv64"
            chost = "riscv64-unknown-linux-gnu"
            flavor = "rv64_lp64d-openrc"
            keyword = "riscv"
            
            [compilation]
            cflags = "-O3 -march=rv64gcv_zvl256b -pipe"
            gcc_version = "16.0.0_p20251005"
            profile = "default/linux/riscv/23.0/rv64/lp64d"
            makeopts = "-j$(nproc) --load-average=$(nproc)"
            emerge_default_opts = "--jobs=$(nproc) --load-average=$(nproc) --quiet-build y"
            
            [repositories]
            opensbi_repo = "https://github.com/cyyself/opensbi"
            opensbi_tag = "k1-opensbi"
            u_boot_repo = "https://gitee.com/bianbu-linux/uboot-2022.10.git"
            u_boot_tag = "k1-bl-v2.2.7-release"
            firmware_repo = "https://gitee.com/bianbu-linux/buildroot-ext.git"
            firmware_tag = "k1-bl-v2.2.7-release"
            kernel_repo = "https://gitee.com/bianbu-linux/linux-6.6.git"
            kernel_tag = "k1-bl-v2.2.7-release"
            bootloader_tag = "k1-bl-v2.2.7-release"
            
            [packages]
            stage1_file = "stage1-packages.txt"
            additional_file = "additional-packages.txt"
            
            [image]
            root_size = "5G"
            boot_size = "500M"
            genimage_config = "genimage-k1.cfg"
        "#;

        std::fs::write(&config_path, config_content).unwrap();

        let config = PlatformConfig::load_from_file(&config_path).unwrap();

        assert_eq!(config.target.arch, "riscv64");
        assert_eq!(config.target.chost, "riscv64-unknown-linux-gnu");
        assert_eq!(
            config.compilation.cflags,
            "-O3 -march=rv64gcv_zvl256b -pipe"
        );
    }

    #[test]
    fn test_load_invalid_config() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("invalid.toml");

        // Test with missing required section
        let invalid_content = r#"
            [compilation]
            cflags = "test"
        "#;

        std::fs::write(&config_path, invalid_content).unwrap();

        let result = PlatformConfig::load_from_file(&config_path);
        assert!(result.is_err());
        // This should fail during parsing since required fields are missing
        assert!(matches!(result, Err(ConfigError::ParseError(_))));
    }

    #[test]
    fn test_load_nonexistent_config() {
        let result = PlatformConfig::load_from_file("/nonexistent/config.toml");
        assert!(result.is_err());
        if let Err(ConfigError::NotFound(msg)) = result {
            assert!(msg.contains("nonexistent"));
        } else {
            panic!("Expected not found error");
        }
    }

    #[test]
    fn test_cross_compile_prefix() {
        let config = PlatformConfig {
            target: TargetConfig {
                arch: "riscv64".to_string(),
                chost: "riscv64-unknown-linux-gnu".to_string(),
                flavor: "test".to_string(),
                keyword: "test".to_string(),
            },
            compilation: CompilationConfig {
                cflags: "test".to_string(),
                gcc_version: "test".to_string(),
                profile: "test".to_string(),
                makeopts: "test".to_string(),
                emerge_default_opts: "test".to_string(),
            },
            repositories: RepositoryConfig {
                opensbi_repo: "test".to_string(),
                opensbi_tag: "test".to_string(),
                u_boot_repo: "test".to_string(),
                u_boot_tag: "test".to_string(),
                firmware_repo: "test".to_string(),
                firmware_tag: "test".to_string(),
                kernel_repo: "test".to_string(),
                kernel_tag: "test".to_string(),
                bootloader_tag: "test".to_string(),
            },
            packages: PackageConfig {
                stage1_file: "test".to_string(),
                additional_file: "test".to_string(),
            },
            image: ImageConfig {
                root_size: "test".to_string(),
                boot_size: "test".to_string(),
                genimage_config: "test".to_string(),
            },
        };

        assert_eq!(config.cross_compile_prefix(), "riscv64-unknown-linux-gnu-");
    }

    #[test]
    fn test_load_package_list() {
        let dir = tempdir().unwrap();
        let package_path = dir.path().join("packages.txt");

        let package_content = r#"
            # This is a comment
            sys-apps/baselayout
            
            sys-apps/portage
            # Another comment
            app-shells/bash
        "#;

        std::fs::write(&package_path, package_content).unwrap();

        let packages = load_package_list(&package_path).unwrap();
        assert_eq!(packages.len(), 3);
        assert_eq!(packages[0], "sys-apps/baselayout");
        assert_eq!(packages[1], "sys-apps/portage");
        assert_eq!(packages[2], "app-shells/bash");
    }
}
