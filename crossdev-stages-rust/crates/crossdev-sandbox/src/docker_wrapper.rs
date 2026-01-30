//! Simple Docker CLI wrapper - no Bollard complexity
//!
//! This module provides a straightforward interface to Docker using the CLI,
//! avoiding the complexity and reliability issues of the Bollard crate.

use crate::{SandboxBackend, SandboxError, SandboxResult};
use log::info;
use std::path::Path;
use std::process::Command;

/// Simple Docker wrapper that uses the docker CLI directly
pub struct DockerWrapper;

impl DockerWrapper {
    /// Check if a Docker image exists, pulling if necessary
    pub fn ensure_image_available(image: &str) -> SandboxResult<()> {
        info!("Checking if image '{}' is available...", image);

        // Check if image exists by trying to inspect it
        let inspect_result = Command::new("docker")
            .args(["inspect", "--type=image", image])
            .output();

        match inspect_result {
            Ok(output) => {
                if output.status.success() {
                    info!("Image '{}' is available", image);
                    return Ok(());
                }
            }
            Err(_) => {}
        }

        // Image not found, pull it
        info!("Image '{}' not found locally, pulling...", image);

        let pull_result = Command::new("docker")
            .args(["pull", image])
            .output()
            .map_err(|e| {
                SandboxError::ContainerCreationFailed(format!(
                    "Failed to execute docker pull: {}",
                    e
                ))
            })?;

        if !pull_result.status.success() {
            let error_msg = String::from_utf8_lossy(&pull_result.stderr);
            return Err(SandboxError::ContainerCreationFailed(format!(
                "Failed to pull image: {}",
                error_msg
            )));
        }

        info!("Successfully pulled image: {}", image);
        Ok(())
    }

    /// Create a Docker container
    pub fn create_container(
        name: &str,
        image: &str,
        command: &[&str],
        working_dir: Option<&Path>,
    ) -> SandboxResult<String> {
        info!("Creating container '{}' with image '{}'", name, image);

        let mut args: Vec<String> = vec![
            "create".to_string(),
            "--name".to_string(),
            name.to_string(),
            image.to_string(),
        ];
        if let Some(wd) = working_dir {
            args.extend(["-w".to_string(), wd.to_string_lossy().into_owned()]);
        }
        args.extend(command.iter().map(|s| s.to_string()));

        let result = Command::new("docker").args(args).output().map_err(|e| {
            SandboxError::ContainerCreationFailed(format!("Failed to create container: {}", e))
        })?;

        if !result.status.success() {
            let error_msg = String::from_utf8_lossy(&result.stderr);
            return Err(SandboxError::ContainerCreationFailed(format!(
                "Failed to create container: {}",
                error_msg
            )));
        }

        let container_id = String::from_utf8_lossy(&result.stdout).trim().to_string();
        info!("Created container: {}", container_id);
        Ok(container_id)
    }

    /// Start a Docker container
    pub fn start_container(name: &str) -> SandboxResult<()> {
        info!("Starting container '{}'", name);

        let result = Command::new("docker")
            .args(["start", name])
            .output()
            .map_err(|e| {
                SandboxError::ContainerCreationFailed(format!("Failed to start container: {}", e))
            })?;

        if !result.status.success() {
            let error_msg = String::from_utf8_lossy(&result.stderr);
            return Err(SandboxError::ContainerCreationFailed(format!(
                "Failed to start container: {}",
                error_msg
            )));
        }

        Ok(())
    }

    /// Execute a command in a running container
    pub fn exec_command(
        container: &str,
        command: &[&str],
        working_dir: Option<&Path>,
    ) -> SandboxResult<String> {
        info!(
            "Executing command in container '{}': {:?}",
            container, command
        );

        let mut args: Vec<String> = vec!["exec".to_string()];
        if let Some(wd) = working_dir {
            args.extend(["-w".to_string(), wd.to_string_lossy().into_owned()]);
        }
        args.push(container.to_string());
        args.extend(command.iter().map(|s| s.to_string()));

        let result = Command::new("docker").args(args).output().map_err(|e| {
            SandboxError::CommandExecutionFailed(format!("Failed to execute command: {}", e))
        })?;

        if !result.status.success() {
            let error_msg = String::from_utf8_lossy(&result.stderr);
            return Err(SandboxError::CommandExecutionFailed(format!(
                "Command failed: {}",
                error_msg
            )));
        }

        let output = String::from_utf8_lossy(&result.stdout).to_string();
        Ok(output)
    }

    /// Remove a Docker container
    pub fn remove_container(name: &str, force: bool) -> SandboxResult<()> {
        info!("Removing container '{}'", name);

        let mut args = vec!["rm"];
        if force {
            args.push("-f");
        }
        args.push(name);

        let result = Command::new("docker").args(args).output().map_err(|e| {
            SandboxError::ContainerCreationFailed(format!("Failed to remove container: {}", e))
        })?;

        if !result.status.success() {
            let error_msg = String::from_utf8_lossy(&result.stderr);
            return Err(SandboxError::ContainerCreationFailed(format!(
                "Failed to remove container: {}",
                error_msg
            )));
        }

        Ok(())
    }

    /// Check if Docker is available
    pub fn is_available() -> bool {
        Command::new("docker")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
}
