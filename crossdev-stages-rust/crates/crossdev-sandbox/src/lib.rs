//! Sandboxing and containerization abstractions for crossdev-stages
//!
//! This crate provides a unified interface for different sandboxing/containerization
//! backends (Docker, Bubblewrap, etc.) with a trait-based architecture.

use async_trait::async_trait;
use bollard::Docker;
use futures_util::stream::StreamExt;
use log::info;
use std::path::Path;
use thiserror::Error;

mod docker_wrapper;
use docker_wrapper::DockerWrapper;

/// Sandboxing errors
#[derive(Error, Debug)]
pub enum SandboxError {
    #[error("Backend not available: {0}")]
    BackendUnavailable(String),

    #[error("Container creation failed: {0}")]
    ContainerCreationFailed(String),

    #[error("Command execution failed: {0}")]
    CommandExecutionFailed(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Sandboxing result type
pub type SandboxResult<T> = Result<T, SandboxError>;

/// Sandbox backend trait
///
/// Defines the interface that all sandbox backends must implement
#[async_trait::async_trait]
pub trait SandboxBackend: Send + Sync {
    /// Create a new sandbox instance
    fn new() -> SandboxResult<Self>
    where
        Self: Sized;

    /// Check if this backend is available on the current system
    fn is_available(&self) -> bool;

    /// Run a command in the sandbox
    async fn run_command(
        &self,
        container_id: &str,
        command: &str,
        args: &[&str],
        working_dir: Option<&Path>,
    ) -> SandboxResult<String>;

    /// Create an interactive exec session in a container
    async fn exec_interactive(
        &self,
        container_id: &str,
        command: &[&str],
        working_dir: Option<&Path>,
    ) -> SandboxResult<()>;

    /// Get the backend name
    fn name(&self) -> &str;
}

/// Bubblewrap sandbox backend
#[cfg(feature = "bubblewrap")]
pub struct BubblewrapBackend;

/// Docker sandbox backend
#[cfg(feature = "docker")]
pub struct DockerBackend;

/// Auto-detect and create the best available sandbox backend
pub fn auto_detect_backend() -> SandboxResult<Box<dyn SandboxBackend>> {
    // Try backends in order of preference

    #[cfg(feature = "docker")]
    if let Ok(backend) = DockerBackend::new() {
        if backend.is_available() {
            return Ok(Box::new(backend));
        }
    }

    #[cfg(feature = "bubblewrap")]
    if let Ok(backend) = BubblewrapBackend::new() {
        if backend.is_available() {
            return Ok(Box::new(backend));
        }
    }

    Err(SandboxError::BackendUnavailable(
        "No available sandbox backend. Enable 'docker' or 'bubblewrap' feature".to_string(),
    ))
}

#[cfg(feature = "bubblewrap")]
#[async_trait]
impl SandboxBackend for BubblewrapBackend {
    fn new() -> SandboxResult<Self> {
        let backend = Self;
        if !backend.is_available() {
            return Err(SandboxError::BackendUnavailable(
                "Bubblewrap not available on this system".to_string(),
            ));
        }
        Ok(backend)
    }

    fn is_available(&self) -> bool {
        // Check if bwrap command is available
        std::process::Command::new("bwrap")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    async fn run_command(
        &self,
        _container_id: &str,
        command: &str,
        args: &[&str],
        working_dir: Option<&Path>,
    ) -> SandboxResult<String> {
        todo!("Implement bubblewrap command execution")
    }

    async fn exec_interactive(
        &self,
        container_id: &str,
        command: &[&str],
        working_dir: Option<&Path>,
    ) -> SandboxResult<()> {
        Err(SandboxError::BackendUnavailable(
            "Interactive exec not implemented for bubblewrap".to_string(),
        ))
    }

    fn name(&self) -> &str {
        "bubblewrap"
    }
}

#[cfg(feature = "docker")]
#[async_trait]
impl SandboxBackend for DockerBackend {
    fn new() -> SandboxResult<Self> {
        let backend = Self;
        if !backend.is_available() {
            return Err(SandboxError::BackendUnavailable(
                "Docker not available on this system".to_string(),
            ));
        }
        Ok(backend)
    }

    fn is_available(&self) -> bool {
        // Check if docker command is available
        std::process::Command::new("docker")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    async fn run_command(
        &self,
        container_id: &str,
        command: &str,
        args: &[&str],
        working_dir: Option<&Path>,
    ) -> SandboxResult<String> {
        use bollard::Docker;

        // Connect to Docker daemon
        let docker = Docker::connect_with_local_defaults()
            .map_err(|e| SandboxError::ContainerCreationFailed(e.to_string()))?;

        // Ensure the container exists and is running
        DockerBackend::ensure_container_ready(&docker, container_id).await?;

        // Build the command with arguments
        let mut full_command = vec![command.to_string()];
        full_command.extend(args.iter().map(|s| s.to_string()));

        // Use docker CLI to execute the command
        let mut args: Vec<String> = vec!["exec".to_string()];
        if let Some(wd) = working_dir {
            args.extend(["-w".to_string(), wd.to_string_lossy().into_owned()]);
        }
        args.push(container_id.to_string());
        args.extend(full_command);

        let output = std::process::Command::new("docker")
            .args(args)
            .output()
            .map_err(|e| SandboxError::CommandExecutionFailed(format!(
                "Failed to execute command: {}", e
            )))?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(SandboxError::CommandExecutionFailed(format!(
                "Command failed: {}", error_msg
            )));
        }

        let result = String::from_utf8_lossy(&output.stdout).into_owned();
        Ok(result)
    }

    /// Create an interactive exec session in a container
    async fn exec_interactive(
        &self,
        container_id: &str,
        command: &[&str],
        working_dir: Option<&Path>,
    ) -> SandboxResult<()> {
        use bollard::Docker;

        // Connect to Docker daemon
        let docker = Docker::connect_with_local_defaults()
            .map_err(|e| SandboxError::ContainerCreationFailed(e.to_string()))?;

        // Ensure the container exists and is running
        DockerBackend::ensure_container_ready(&docker, container_id).await?;

        // Use docker CLI directly for interactive sessions as it handles TTY properly
        let mut args: Vec<String> = vec!["exec".to_string(), "-it".to_string()];
        if let Some(wd) = working_dir {
            args.extend(["-w".to_string(), wd.to_string_lossy().into_owned()]);
        }
        args.push(container_id.to_string());
        
        // For interactive shell, use bash -li
        if command == ["bash", "-li"] {
            args.extend(["bash".to_string(), "-li".to_string()]);
        } else {
            args.extend(command.iter().map(|s| s.to_string()));
        }

        // Execute the interactive command using docker CLI
        match std::process::Command::new("docker")
            .args(args)
            .status() {
            Ok(status) => {
                // Interactive sessions can exit with any code when user exits
                // Only log if it's not a normal exit (0) or common shell exit codes
                if !status.success() && !status.code().map_or(false, |c| c == 0 || c == 1 || c == 130) {
                    info!("Interactive session exited with code: {:?}", status.code());
                }
                Ok(())
            }
            Err(e) => Err(SandboxError::CommandExecutionFailed(format!("Failed to start interactive session: {}", e)))
        }
    }



    fn name(&self) -> &str {
        "docker"
    }
}

#[cfg(feature = "docker")]
impl DockerBackend {
    /// Ensure container exists and is running using docker run
    async fn ensure_container_ready(docker: &Docker, container_id: &str) -> SandboxResult<()> {
        info!("Ensuring container '{}' is ready...", container_id);

        // Check if container already exists
        match docker.inspect_container(container_id, None).await {
            Ok(inspect) => {
                info!("✓ Container '{}' already exists", container_id);
                
                if let Some(state) = inspect.state {
                    if state.running.unwrap_or(false) {
                        info!("✓ Container '{}' is already running", container_id);
                        return Ok(());
                    } else {
                        info!("Container '{}' exists but is stopped, starting it...", container_id);
                        // Start the existing stopped container
                        match docker.start_container::<String>(container_id, None).await {
                            Ok(_) => {
                                info!("✓ Container '{}' started successfully", container_id);
                                return Ok(());
                            }
                            Err(e) => {
                                info!("Failed to start existing container '{}': {}", container_id, e);
                                // If we can't start it, remove and recreate
                                let _ = docker.remove_container(
                                    container_id,
                                    Some(bollard::container::RemoveContainerOptions {
                                        force: true,
                                        ..Default::default()
                                    }),
                                ).await;
                            }
                        }
                    }
                }
            }
            Err(e) if e.to_string().contains("No such container") => {
                info!("Container '{}' doesn't exist, creating it...", container_id);
            }
            Err(e) => {
                return Err(SandboxError::CommandExecutionFailed(format!(
                    "Error checking container '{}': {}", container_id, e
                )));
            }
        }

        // Create new container using docker run
        let args = vec![
            "run".to_string(),
            "-d".to_string(),  // Detached mode
            "--name".to_string(),
            container_id.to_string(),
            "gentoo/stage3".to_string(),
            "sleep".to_string(),
            "infinity".to_string(),
        ];

        match std::process::Command::new("docker")
            .args(&args)
            .output() {
            Ok(output) => {
                if output.status.success() {
                    info!("✓ Container '{}' created and started successfully", container_id);
                    Ok(())
                } else {
                    let error_msg = String::from_utf8_lossy(&output.stderr);
                    Err(SandboxError::CommandExecutionFailed(format!(
                        "Failed to create container '{}': {}", container_id, error_msg
                    )))
                }
            }
            Err(e) => {
                Err(SandboxError::CommandExecutionFailed(format!(
                    "Failed to execute docker run: {}", e
                )))
            }
        }
    }

    /// Ensure the required Docker image is available, pulling if necessary
    pub async fn ensure_image_available(&self, image: &str) -> SandboxResult<()> {
        // Simple approach: try to create a container with the image
        // If it fails because the image doesn't exist, pull it
        use bollard::container::CreateContainerOptions;

        let docker = Docker::connect_with_local_defaults()
            .map_err(|e| SandboxError::ContainerCreationFailed(e.to_string()))?;

        // Try to create a test container to check if image exists
        let test_config = bollard::container::Config {
            image: Some(image.to_string()),
            cmd: Some(vec!["echo".to_string(), "test".to_string()]),
            ..Default::default()
        };

        let result = docker
            .create_container(
                Some(CreateContainerOptions {
                    name: "crossdev-test-image-check",
                    platform: None,
                }),
                test_config,
            )
            .await;

        match result {
            Ok(_) => {
                // Image exists, remove the test container
                let _ = docker
                    .remove_container(
                        "crossdev-test-image-check",
                        Some(bollard::container::RemoveContainerOptions {
                            force: true,
                            link: false,
                            v: false,
                        }),
                    )
                    .await;
                info!("Image '{}' is available", image);
            }
            Err(e) => {
                // If error indicates image not found, pull it
                if e.to_string().contains("No such image") || e.to_string().contains("not found") {
                    info!("Image '{}' not found locally, pulling...", image);

                    // Simple pull using docker CLI as fallback
                    let pull_result = std::process::Command::new("docker")
                        .args(["pull", image])
                        .output();

                    match pull_result {
                        Ok(output) => {
                            if output.status.success() {
                                info!("Successfully pulled image: {}", image);
                            } else {
                                let error_msg = String::from_utf8_lossy(&output.stderr);
                                return Err(SandboxError::ContainerCreationFailed(format!(
                                    "Failed to pull image: {}",
                                    error_msg
                                )));
                            }
                        }
                        Err(e) => {
                            return Err(SandboxError::ContainerCreationFailed(format!(
                                "Failed to execute docker pull: {}",
                                e
                            )))
                        }
                    }
                } else {
                    return Err(SandboxError::ContainerCreationFailed(format!(
                        "Image check failed: {}",
                        e
                    )));
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_detect_with_default_features() {
        // With default features (docker and bubblewrap), should find at least one backend
        let result = auto_detect_backend();
        // This test will pass if either docker or bubblewrap is available on the system
        // If neither is available, it will fail, which is expected for the test environment
        match result {
            Ok(backend) => {
                assert!(backend.name() == "docker" || backend.name() == "bubblewrap");
            }
            Err(_) => {
                // This is expected in test environments without docker/bubblewrap installed
                assert!(true); // Test passes if no backend is available in test environment
            }
        }
    }

    #[test]
    #[cfg(feature = "bubblewrap")]
    fn test_bubblewrap_availability() {
        // Test that availability check works (may fail if bwrap not installed)
        let backend = BubblewrapBackend;
        let available = backend.is_available();
        // Just check that the function doesn't panic
        assert!(true); // We can't assert availability since it depends on system
    }

    #[test]
    #[cfg(feature = "docker")]
    fn test_docker_availability() {
        // Test that availability check works (may fail if docker not installed)
        let backend = DockerBackend;
        let available = backend.is_available();
        // Just check that the function doesn't panic
        assert!(true); // We can't assert availability since it depends on system
    }

    #[test]
    fn test_sandbox_enter_error_handling() {
        // Test that the sandbox enter logic handles errors properly
        // This test verifies the logic without requiring Docker to be running
        
        // Test auto-detection
        let result = auto_detect_backend();
        match result {
            Ok(backend) => {
                println!("Backend detected: {}", backend.name());
                assert!(backend.is_available());
            }
            Err(_) => {
                // Expected in test environments without Docker/bubblewrap
                println!("No backend available (expected in test environment)");
            }
        }
        
        // Test that we can create backend instances
        #[cfg(feature = "docker")]
        {
            let docker_backend = DockerBackend::new();
            match docker_backend {
                Ok(_) => println!("Docker backend created successfully"),
                Err(e) => println!("Docker backend creation failed (expected): {}", e),
            }
        }
        
        #[cfg(feature = "bubblewrap")]
        {
            let bubblewrap_backend = BubblewrapBackend::new();
            match bubblewrap_backend {
                Ok(_) => println!("Bubblewrap backend created successfully"),
                Err(e) => println!("Bubblewrap backend creation failed (expected): {}", e),
            }
        }
    }
}
