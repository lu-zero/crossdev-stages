//! Sandboxing and containerization abstractions for crossdev-stages
//!
//! This crate provides a unified interface for different sandboxing/containerization
//! backends (Docker, Bubblewrap, etc.) with a trait-based architecture.

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
}

/// Sandboxing result type
pub type SandboxResult<T> = Result<T, SandboxError>;

/// Sandbox backend trait
///
/// Defines the interface that all sandbox backends must implement
pub trait SandboxBackend: Send + Sync {
    /// Create a new sandbox instance
    fn new() -> SandboxResult<Self> where Self: Sized;

    /// Check if this backend is available on the current system
    fn is_available(&self) -> bool;

    /// Run a command in the sandbox
    fn run_command(
        &self,
        command: &str,
        args: &[&str],
        working_dir: Option<&Path>,
    ) -> SandboxResult<String>;

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
        "No available sandbox backend. Enable 'docker' or 'bubblewrap' feature".to_string()
    ))
}

#[cfg(feature = "bubblewrap")]
impl SandboxBackend for BubblewrapBackend {
    fn new() -> SandboxResult<Self> {
        let backend = Self;
        if !backend.is_available() {
            return Err(SandboxError::BackendUnavailable(
                "Bubblewrap not available on this system".to_string()
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

    fn run_command(
        &self,
        command: &str,
        args: &[&str],
        working_dir: Option<&Path>,
    ) -> SandboxResult<String> {
        todo!("Implement bubblewrap command execution")
    }

    fn name(&self) -> &str {
        "bubblewrap"
    }
}

#[cfg(feature = "docker")]
impl SandboxBackend for DockerBackend {
    fn new() -> SandboxResult<Self> {
        let backend = Self;
        if !backend.is_available() {
            return Err(SandboxError::BackendUnavailable(
                "Docker not available on this system".to_string()
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

    fn run_command(
        &self,
        command: &str,
        args: &[&str],
        working_dir: Option<&Path>,
    ) -> SandboxResult<String> {
        todo!("Implement Docker command execution")
    }

    fn name(&self) -> &str {
        "docker"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_detect_no_features() {
        // When no features are enabled, should return error
        let result = auto_detect_backend();
        assert!(result.is_err());
        if let Err(SandboxError::BackendUnavailable(msg)) = result {
            assert!(msg.contains("No available sandbox backend"));
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
}
