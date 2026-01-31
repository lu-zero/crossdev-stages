//! Stage3 registry and management

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Information about a registered stage3
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stage3Info {
    /// Name of the stage3 (e.g., "riscv64-k1")
    pub name: String,
    
    /// Path to the stage3 root directory
    pub path: PathBuf,
    
    /// Target architecture
    pub target: String,
    
    /// Version/timestamp of the stage3
    pub version: String,
    
    /// Status (ready, updating, etc.)
    pub status: String,
    
    /// Last update timestamp
    pub last_updated: String,
}

/// Stage3 registry
#[derive(Debug, Default)]
pub struct Stage3Registry {
    stage3s: Vec<Stage3Info>,
}

/// Registry errors
#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("Registry file not found: {0}")]
    NotFound(String),
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("Parse error: {0}")]
    ParseError(String),
    
    #[error("Stage3 not found: {0}")]
    Stage3NotFound(String),
    
    #[error("Stage3 already exists: {0}")]
    Stage3Exists(String),
}

impl Stage3Registry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self { stage3s: Vec::new() }
    }
    
    /// Load registry from file
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, RegistryError> {
        let path = path.as_ref();
        
        // Check if file exists
        if !path.exists() {
            return Ok(Self::new()); // Return empty registry if file doesn't exist
        }
        
        // Read file content
        let content = std::fs::read_to_string(path)
            .map_err(|e| RegistryError::IoError(e))?;
        
        // Parse TOML
        let registry: Stage3Registry = toml::from_str(&content)
            .map_err(|e| RegistryError::ParseError(e.to_string()))?;
        
        Ok(registry)
    }
    
    /// Save registry to file
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), RegistryError> {
        let content = toml::to_string(self)
            .map_err(|e| RegistryError::ParseError(e.to_string()))?;
        
        std::fs::write(path, content)
            .map_err(|e| RegistryError::IoError(e))?;
        
        Ok(())
    }
    
    /// Add a stage3 to the registry
    pub fn add_stage3(&mut self, stage3: Stage3Info) -> Result<(), RegistryError> {
        // Check if already exists
        if self.stage3s.iter().any(|s| s.name == stage3.name) {
            return Err(RegistryError::Stage3Exists(stage3.name));
        }
        
        self.stage3s.push(stage3);
        Ok(())
    }
    
    /// Remove a stage3 from the registry
    pub fn remove_stage3(&mut self, name: &str) -> Result<Stage3Info, RegistryError> {
        let pos = self.stage3s.iter()
            .position(|s| s.name == name)
            .ok_or_else(|| RegistryError::Stage3NotFound(name.to_string()))?;
        
        Ok(self.stage3s.remove(pos))
    }
    
    /// Get a stage3 by name
    pub fn get_stage3(&self, name: &str) -> Option<&Stage3Info> {
        self.stage3s.iter().find(|s| s.name == name)
    }
    
    /// List all stage3s
    pub fn list_stage3s(&self) -> &[Stage3Info] {
        &self.stage3s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    
    #[test]
    fn test_empty_registry() {
        let registry = Stage3Registry::new();
        assert_eq!(registry.list_stage3s().len(), 0);
    }
    
    #[test]
    fn test_add_remove_stage3() {
        let mut registry = Stage3Registry::new();
        
        let stage3 = Stage3Info {
            name: "riscv64-k1".to_string(),
            path: PathBuf::from("/tmp/test"),
            target: "riscv64-k1".to_string(),
            version: "20240130".to_string(),
            status: "ready".to_string(),
            last_updated: "2024-01-30T12:00:00Z".to_string(),
        };
        
        registry.add_stage3(stage3.clone()).unwrap();
        assert_eq!(registry.list_stage3s().len(), 1);
        
        let removed = registry.remove_stage3("riscv64-k1").unwrap();
        assert_eq!(removed.name, "riscv64-k1");
        assert_eq!(registry.list_stage3s().len(), 0);
    }
    
    #[test]
    fn test_load_save_registry() {
        let dir = tempdir().unwrap();
        let registry_path = dir.path().join("registry.toml");
        
        let mut registry = Stage3Registry::new();
        registry.add_stage3(Stage3Info {
            name: "test".to_string(),
            path: PathBuf::from("/tmp/test"),
            target: "test".to_string(),
            version: "1.0".to_string(),
            status: "ready".to_string(),
            last_updated: "2024-01-30T12:00:00Z".to_string(),
        }).unwrap();
        
        registry.save_to_file(&registry_path).unwrap();
        
        let loaded = Stage3Registry::load_from_file(&registry_path).unwrap();
        assert_eq!(loaded.list_stage3s().len(), 1);
        assert_eq!(loaded.get_stage3("test").unwrap().name, "test");
    }
}
