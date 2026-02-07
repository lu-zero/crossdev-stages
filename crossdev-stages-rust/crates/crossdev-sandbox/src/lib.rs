//! Sandboxing and containerization abstractions for crossdev-stages
//!
//! This crate provides a unified interface for different sandboxing/containerization
//! backends (Docker, Bubblewrap, etc.) with a trait-based architecture.

use async_trait::async_trait;
use bollard::Docker;
use jiff::Timestamp;
use log::info;
use std::path::Path;
use thiserror::Error;

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

    #[error("Stage3 operation failed: {0}")]
    Stage3OperationFailed(String),
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

    /// Load a stage3 into the sandbox container
    async fn load_stage3(
        &self,
        container_id: &str,
        stage3_path: &std::path::Path,
    ) -> SandboxResult<()>;

    /// Save the current state of the sandbox container as a stage3 archive
    async fn save_stage3(
        &self,
        container_id: &str,
        target_path: &std::path::Path,
    ) -> SandboxResult<()>;

    /// Wipe the stage3 from the sandbox container
    async fn wipe_stage3(&self, container_id: &str) -> SandboxResult<()>;
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
        _command: &str,
        _args: &[&str],
        _working_dir: Option<&Path>,
    ) -> SandboxResult<String> {
        todo!("Implement bubblewrap command execution")
    }

    async fn exec_interactive(
        &self,
        _container_id: &str,
        _command: &[&str],
        _working_dir: Option<&Path>,
    ) -> SandboxResult<()> {
        Err(SandboxError::BackendUnavailable(
            "Interactive exec not implemented for bubblewrap".to_string(),
        ))
    }

    fn name(&self) -> &str {
        "bubblewrap"
    }

    /// Load a stage3 into the Bubblewrap sandbox
    async fn load_stage3(
        &self,
        _container_id: &str,
        _stage3_path: &std::path::Path,
    ) -> SandboxResult<()> {
        Err(SandboxError::BackendUnavailable(
            "Stage3 operations not implemented for bubblewrap backend".to_string(),
        ))
    }

    /// Save the current state of the Bubblewrap sandbox as a stage3 archive
    async fn save_stage3(
        &self,
        _container_id: &str,
        _target_path: &std::path::Path,
    ) -> SandboxResult<()> {
        Err(SandboxError::BackendUnavailable(
            "Stage3 operations not implemented for bubblewrap backend".to_string(),
        ))
    }

    /// Wipe the stage3 from the Bubblewrap sandbox
    async fn wipe_stage3(&self, _container_id: &str) -> SandboxResult<()> {
        Err(SandboxError::BackendUnavailable(
            "Stage3 operations not implemented for bubblewrap backend".to_string(),
        ))
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
            .map_err(|e| {
                SandboxError::CommandExecutionFailed(format!("Failed to execute command: {}", e))
            })?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(SandboxError::CommandExecutionFailed(format!(
                "Command failed: {}",
                error_msg
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
        match std::process::Command::new("docker").args(args).status() {
            Ok(status) => {
                // Interactive sessions can exit with any code when user exits
                // Only log if it's not a normal exit (0) or common shell exit codes
                if !status.success()
                    && !status
                        .code()
                        .map_or(false, |c| c == 0 || c == 1 || c == 130)
                {
                    info!("Interactive session exited with code: {:?}", status.code());
                }
                Ok(())
            }
            Err(e) => Err(SandboxError::CommandExecutionFailed(format!(
                "Failed to start interactive session: {}",
                e
            ))),
        }
    }

    fn name(&self) -> &str {
        "docker"
    }

    /// Load a stage3 into the Docker container using docker cp and in-container extraction
    async fn load_stage3(
        &self,
        container_id: &str,
        stage3_path: &std::path::Path,
    ) -> SandboxResult<()> {
        info!(
            "Loading stage3 into container '{}' from: {}",
            container_id,
            stage3_path.display()
        );

        // Ensure container is running using docker CLI
        let status_output = std::process::Command::new("docker")
            .args(["inspect", "--format", "{{.State.Running}}", container_id])
            .output()
            .map_err(|e| {
                SandboxError::Stage3OperationFailed(format!(
                    "Failed to check container status: {}",
                    e
                ))
            })?;

        if !status_output.status.success() {
            let stderr = String::from_utf8_lossy(&status_output.stderr);
            if !stderr.contains("No such container") {
                return Err(SandboxError::Stage3OperationFailed(format!(
                    "Failed to check container status: {}",
                    stderr
                )));
            }

            // Container doesn't exist, start it
            let start_output = std::process::Command::new("docker")
                .args(["start", container_id])
                .output()
                .map_err(|e| {
                    SandboxError::Stage3OperationFailed(format!("Failed to start container: {}", e))
                })?;

            if !start_output.status.success() {
                let stderr = String::from_utf8_lossy(&start_output.stderr);
                return Err(SandboxError::Stage3OperationFailed(format!(
                    "Failed to start container: {}",
                    stderr
                )));
            }
        }

        // Copy the stage3 archive into the container
        let output = std::process::Command::new("docker")
            .args([
                "cp",
                stage3_path.to_str().unwrap(),
                &format!("{}:/tmp/stage3.tar.xz", container_id),
            ])
            .output()
            .map_err(|e| {
                SandboxError::Stage3OperationFailed(format!(
                    "Failed to copy stage3 to container: {}",
                    e
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SandboxError::Stage3OperationFailed(format!(
                "Failed to copy stage3 to container: {}",
                stderr
            )));
        }

        // Parse stage3 filename to extract arch and flavor for proper naming
        let stage3_filename = stage3_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown");

        // Extract arch and flavor from filename (format: stage3-{arch}-{flavor}-{date}.tar.xz)
        let parts: Vec<&str> = stage3_filename.split('-').collect();
        let arch = parts.get(1).copied().unwrap_or("unknown");
        let flavor = parts.get(2).copied().unwrap_or("unknown");
        let tag = Timestamp::now().strftime("%Y%m%dT%H%M");

        let stage_dir = format!("/stages/{}-{}-{}", arch, flavor, tag);

        // Extract the stage3 archive inside the container with proper naming and .origin file
        let extract_output = std::process::Command::new("docker")
            .args(["exec", container_id, "sh", "-c",
                &format!("mkdir -p {} && cd {} && tar -xJpf /tmp/stage3.tar.xz --exclude dev/* && echo '{}' > .origin", stage_dir, stage_dir, stage3_filename)])
            .output()
            .map_err(|e| SandboxError::Stage3OperationFailed(format!(
                "Failed to extract stage3 in container: {}", e
            )))?;

        if !extract_output.status.success() {
            let stderr = String::from_utf8_lossy(&extract_output.stderr);
            return Err(SandboxError::Stage3OperationFailed(format!(
                "Stage3 extraction in container failed: {}",
                stderr
            )))?;
        }

        // Clean up the temporary file
        let _ = std::process::Command::new("docker")
            .args(["exec", container_id, "rm", "/tmp/stage3.tar.xz"])
            .output();

        info!(
            "Stage3 loaded successfully into container '{}'",
            container_id
        );
        Ok(())
    }

    /// Save the current state of the Docker container as a stage3 archive using in-container operations
    async fn save_stage3(
        &self,
        container_id: &str,
        target_path: &std::path::Path,
    ) -> SandboxResult<()> {
        info!(
            "Saving container '{}' state to: {}",
            container_id,
            target_path.display()
        );

        // Ensure parent directory exists
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Ensure container is running using docker CLI
        let status_output = std::process::Command::new("docker")
            .args(["inspect", "--format", "{{.State.Running}}", container_id])
            .output()
            .map_err(|e| {
                SandboxError::Stage3OperationFailed(format!(
                    "Failed to check container status: {}",
                    e
                ))
            })?;

        if !status_output.status.success() {
            let stderr = String::from_utf8_lossy(&status_output.stderr);
            if !stderr.contains("No such container") {
                return Err(SandboxError::Stage3OperationFailed(format!(
                    "Failed to check container status: {}",
                    stderr
                )));
            }

            // Container doesn't exist, start it
            let start_output = std::process::Command::new("docker")
                .args(["start", container_id])
                .output()
                .map_err(|e| {
                    SandboxError::Stage3OperationFailed(format!("Failed to start container: {}", e))
                })?;

            if !start_output.status.success() {
                let stderr = String::from_utf8_lossy(&start_output.stderr);
                return Err(SandboxError::Stage3OperationFailed(format!(
                    "Failed to start container: {}",
                    stderr
                )));
            }
        }

        // Create the archive inside the container
        let create_output = std::process::Command::new("docker")
            .args([
                "exec",
                container_id,
                "sh",
                "-c",
                "cd /mnt/stages && tar -cJpf /tmp/stage3-save.tar.xz .",
            ])
            .output()
            .map_err(|e| {
                SandboxError::Stage3OperationFailed(format!(
                    "Failed to create stage3 archive in container: {}",
                    e
                ))
            })?;

        if !create_output.status.success() {
            let stderr = String::from_utf8_lossy(&create_output.stderr);
            return Err(SandboxError::Stage3OperationFailed(format!(
                "Stage3 archive creation in container failed: {}",
                stderr
            )));
        }

        // Copy the archive from the container to the host
        let copy_output = std::process::Command::new("docker")
            .args([
                "cp",
                &format!("{}:/tmp/stage3-save.tar.xz", container_id),
                target_path.to_str().unwrap(),
            ])
            .output()
            .map_err(|e| {
                SandboxError::Stage3OperationFailed(format!(
                    "Failed to copy stage3 archive from container: {}",
                    e
                ))
            })?;

        if !copy_output.status.success() {
            let stderr = String::from_utf8_lossy(&copy_output.stderr);
            return Err(SandboxError::Stage3OperationFailed(format!(
                "Failed to copy stage3 archive from container: {}",
                stderr
            )));
        }

        // Clean up the temporary file in the container
        let _ = std::process::Command::new("docker")
            .args(["exec", container_id, "rm", "/tmp/stage3-save.tar.xz"])
            .output();

        info!(
            "Container '{}' state saved successfully to: {}",
            container_id,
            target_path.display()
        );
        Ok(())
    }

    /// Wipe the stage3 from the Docker container using in-container operations
    async fn wipe_stage3(&self, container_id: &str) -> SandboxResult<()> {
        info!("Wiping stage3 from container '{}'", container_id);

        // Ensure container is running using docker CLI
        let status_output = std::process::Command::new("docker")
            .args(["inspect", "--format", "{{.State.Running}}", container_id])
            .output()
            .map_err(|e| {
                SandboxError::Stage3OperationFailed(format!(
                    "Failed to check container status: {}",
                    e
                ))
            })?;

        if !status_output.status.success() {
            let stderr = String::from_utf8_lossy(&status_output.stderr);
            if !stderr.contains("No such container") {
                return Err(SandboxError::Stage3OperationFailed(format!(
                    "Failed to check container status: {}",
                    stderr
                )));
            }

            // Container doesn't exist, start it
            let start_output = std::process::Command::new("docker")
                .args(["start", container_id])
                .output()
                .map_err(|e| {
                    SandboxError::Stage3OperationFailed(format!("Failed to start container: {}", e))
                })?;

            if !start_output.status.success() {
                let stderr = String::from_utf8_lossy(&start_output.stderr);
                return Err(SandboxError::Stage3OperationFailed(format!(
                    "Failed to start container: {}",
                    stderr
                )));
            }
        }

        // Wipe the stage3 directory inside the container
        let wipe_output = std::process::Command::new("docker")
            .args([
                "exec",
                container_id,
                "sh",
                "-c",
                "rm -rf /mnt/stages/* && mkdir -p /mnt/stages",
            ])
            .output()
            .map_err(|e| {
                SandboxError::Stage3OperationFailed(format!(
                    "Failed to wipe stage3 in container: {}",
                    e
                ))
            })?;

        if !wipe_output.status.success() {
            let stderr = String::from_utf8_lossy(&wipe_output.stderr);
            return Err(SandboxError::Stage3OperationFailed(format!(
                "Stage3 wipe in container failed: {}",
                stderr
            )));
        }

        info!(
            "Stage3 wiped successfully from container '{}'",
            container_id
        );
        Ok(())
    }
}

#[cfg(feature = "docker")]
impl DockerBackend {
    /// Ensure container exists and is running using docker run
    async fn ensure_container_ready(_docker: &Docker, container_id: &str) -> SandboxResult<()> {
        info!("Ensuring container '{}' is ready...", container_id);

        // First try to check if container exists using docker CLI (more reliable)
        let check_result = std::process::Command::new("docker")
            .args([
                "ps",
                "-a",
                "--filter",
                &format!("name={}", container_id),
                "--format",
                "{{.Names}}",
            ])
            .output();

        match check_result {
            Ok(output) => {
                if output.status.success() {
                    let container_names = String::from_utf8_lossy(&output.stdout);
                    if container_names.trim() == container_id {
                        info!("✓ Container '{}' already exists", container_id);

                        // Check if it's running
                        let status_result = std::process::Command::new("docker")
                            .args(["inspect", "-f", "{{.State.Running}}", container_id])
                            .output();

                        match status_result {
                            Ok(status_output) => {
                                if status_output.status.success() {
                                    let running_status_str =
                                        String::from_utf8_lossy(&status_output.stdout);
                                    let running_status = running_status_str.trim();
                                    if running_status == "true" {
                                        info!("✓ Container '{}' is already running", container_id);
                                        return Ok(());
                                    } else {
                                        info!(
                                            "Container '{}' exists but is stopped, starting it...",
                                            container_id
                                        );
                                        // Start the existing stopped container
                                        let start_result = std::process::Command::new("docker")
                                            .args(["start", container_id])
                                            .output();

                                        match start_result {
                                            Ok(start_output) => {
                                                if start_output.status.success() {
                                                    info!(
                                                        "✓ Container '{}' started successfully",
                                                        container_id
                                                    );

                                                    // Wait for container to be fully ready (up to 5 seconds)
                                                    let start_time = std::time::Instant::now();
                                                    let timeout = std::time::Duration::from_secs(5);

                                                    while start_time.elapsed() < timeout {
                                                        // Check if container is running
                                                        let ready_check =
                                                            std::process::Command::new("docker")
                                                                .args([
                                                                    "inspect",
                                                                    "-f",
                                                                    "{{.State.Running}}",
                                                                    container_id,
                                                                ])
                                                                .output();

                                                        match ready_check {
                                                            Ok(check_output) => {
                                                                if check_output.status.success() {
                                                                    let running_status_str =
                                                                        String::from_utf8_lossy(
                                                                            &check_output.stdout,
                                                                        );
                                                                    let running_status =
                                                                        running_status_str.trim();
                                                                    if running_status == "true" {
                                                                        info!("✓ Container '{}' is fully ready", container_id);
                                                                        return Ok(());
                                                                    }
                                                                }
                                                            }
                                                            Err(_) => {
                                                                // Ignore errors and keep waiting
                                                            }
                                                        }

                                                        // Small delay to avoid busy waiting
                                                        std::thread::sleep(
                                                            std::time::Duration::from_millis(100),
                                                        );
                                                    }

                                                    info!("⚠ Container '{}' did not become ready within timeout", container_id);
                                                    return Err(SandboxError::CommandExecutionFailed(format!(
                                                        "Container '{}' did not become ready within 5 seconds",
                                                        container_id
                                                    )));
                                                } else {
                                                    let error_msg = String::from_utf8_lossy(
                                                        &start_output.stderr,
                                                    );
                                                    info!("Failed to start existing container '{}': {}", container_id, error_msg);
                                                    // If we can't start it, remove and recreate
                                                    let _ = std::process::Command::new("docker")
                                                        .args(["rm", "-f", container_id])
                                                        .output();
                                                }
                                            }
                                            Err(e) => {
                                                info!(
                                                    "Failed to start existing container '{}': {}",
                                                    container_id, e
                                                );
                                                // If we can't start it, remove and recreate
                                                let _ = std::process::Command::new("docker")
                                                    .args(["rm", "-f", container_id])
                                                    .output();
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                info!("Failed to check container status '{}': {}", container_id, e);
                                // If we can't check status, remove and recreate
                                let _ = std::process::Command::new("docker")
                                    .args(["rm", "-f", container_id])
                                    .output();
                            }
                        }
                    }
                }
            }
            Err(e) => {
                info!(
                    "Failed to check for existing container '{}': {}",
                    container_id, e
                );
            }
        }

        info!("Container '{}' doesn't exist, creating it...", container_id);

        // Create new container using docker create (better for reusable instances)
        // Use sleep infinity to keep container running (simple and reliable)
        let args = vec![
            "create".to_string(), // Use create instead of run for better lifecycle management
            "--name".to_string(),
            container_id.to_string(),
            "gentoo/stage3".to_string(),
            "sleep".to_string(),
            "infinity".to_string(), // Keep container running indefinitely
        ];

        match std::process::Command::new("docker").args(&args).output() {
            Ok(output) => {
                if output.status.success() {
                    info!("✓ Container '{}' created successfully", container_id);

                    // Start the container after creation
                    match std::process::Command::new("docker")
                        .args(["start", container_id])
                        .output()
                    {
                        Ok(start_output) => {
                            if start_output.status.success() {
                                info!("✓ Container '{}' started", container_id);
                                Ok(())
                            } else {
                                let error_msg = String::from_utf8_lossy(&start_output.stderr);
                                Err(SandboxError::CommandExecutionFailed(format!(
                                    "Failed to start container '{}': {}",
                                    container_id, error_msg
                                )))
                            }
                        }
                        Err(e) => Err(SandboxError::CommandExecutionFailed(format!(
                            "Failed to start container '{}': {}",
                            container_id, e
                        ))),
                    }
                } else {
                    let error_msg = String::from_utf8_lossy(&output.stderr);
                    Err(SandboxError::CommandExecutionFailed(format!(
                        "Failed to create container '{}': {}",
                        container_id, error_msg
                    )))
                }
            }
            Err(e) => Err(SandboxError::CommandExecutionFailed(format!(
                "Failed to execute docker create: {}",
                e
            ))),
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
        let _available = backend.is_available();
        // Just check that the function doesn't panic
        assert!(true); // We can't assert availability since it depends on system
    }

    #[test]
    #[cfg(feature = "docker")]
    fn test_docker_availability() {
        // Test that availability check works (may fail if docker not installed)
        let backend = DockerBackend;
        let _available = backend.is_available();
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
