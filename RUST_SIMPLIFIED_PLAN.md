# Simplified Rust Porting Plan for crossdev-stages

## Updated Architecture Overview

Based on feedback, I've simplified the approach to focus on:
1. **Simpler Git handling** - Use Git CLI instead of git2 crate
2. **Dedicated stage3 fetcher** - Extract complex image fetching logic

### Revised Workspace Layout

```
crossdev-stages-rust/
├── Cargo.toml                  # Workspace root
├── crates/
│   ├── crossdev-config/        # Configuration management
│   ├── crossdev-core/          # Core cross-compilation logic
│   ├── crossdev-image/         # Image building
│   ├── crossdev-stage3/        # Stage3 image fetching (NEW)
│   ├── crossdev-utils/         # System utilities
│   └── crossdev-cli/           # CLI interface
├── config/                     # Configuration files
├── docs/                       # Documentation
└── tests/                      # Integration tests
```

## Key Changes from Original Plan

### 1. Simplified Git Handling

**Before**: Use `git2` crate for repository management
**After**: Use Git CLI commands via `command-group`

**Rationale**:
- Reduces dependency complexity
- Maintains compatibility with existing workflows
- Easier to debug and understand
- Avoids dealing with git2's complexity

### 2. Dedicated Stage3 Fetcher

**Before**: Stage3 fetching logic mixed in with other functionality
**After**: Dedicated `crossdev-stage3` crate

**Rationale**:
- Complex logic deserves its own crate
- Can be reused by other tools
- Easier to test and maintain
- Clear separation of concerns

## Updated Crate Responsibilities

### crossdev-stage3 (NEW)

**Purpose**: Stage3 image fetching and management

**Responsibilities**:
- Fetch Gentoo stage3 images
- Parse stage3 metadata
- Handle image verification
- Manage local stage3 cache
- Provide stage3 information

**Dependencies**:
- `crossdev-config` - Configuration
- `thiserror` - Error handling
- `log` - Logging
- `command-group` - Command execution
- `reqwest` - HTTP requests (optional)
- `tokio` - Async operations

### crossdev-image (Updated)

**Purpose**: Image building (simplified)

**Responsibilities**:
- Source repository management (via Git CLI)
- Build process orchestration
- Filesystem creation
- Image generation with genimage

**Removed**:
- Git repository management (now uses Git CLI)
- Complex stage3 fetching logic

**Dependencies**:
- `crossdev-config` - Configuration
- `crossdev-stage3` - Stage3 fetching
- `thiserror` - Error handling
- `log` - Logging
- `command-group` - Git CLI execution
- `tokio` - Async operations

## Detailed Implementation

### 1. Simplified Git Repository Management

```rust
// crossdev-image/src/repositories.rs
use std::process::Command;
use command_group::CommandGroup;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("Git operation failed: {0}")]
    GitError(String),
    #[error("Repository not found: {0}")]
    NotFound(String),
}

pub struct RepositoryManager {
    build_dir: String,
}

impl RepositoryManager {
    pub fn new(build_dir: &str) -> Self {
        Self {
            build_dir: build_dir.to_string(),
        }
    }

    pub fn checkout_all(&self, config: &RepositoryConfig) -> Result<(), RepositoryError> {
        self.checkout_repo(
            &config.opensbi_repo,
            &config.opensbi_tag,
            "opensbi"
        )?;
        
        self.checkout_repo(
            &config.u_boot_repo,
            &config.bootloader_tag,
            "u-boot"
        )?;
        
        // ... other repositories
        
        Ok(())
    }

    fn checkout_repo(&self, repo_url: &str, tag: &str, dest: &str) -> Result<(), RepositoryError> {
        let dest_path = Path::new(&self.build_dir).join(dest);
        
        if dest_path.exists() {
            // Update existing repository
            self.git_fetch_checkout(repo_url, tag, &dest_path)?;
        } else {
            // Clone new repository
            self.git_clone_checkout(repo_url, tag, &dest_path)?;
        }
        
        Ok(())
    }

    fn git_clone_checkout(&self, repo_url: &str, tag: &str, dest_path: &Path) -> Result<(), RepositoryError> {
        let mut group = CommandGroup::new();
        
        // Clone repository
        group.command(
            Command::new("git")
                .arg("clone")
                .arg("--depth").arg("1")
                .arg("--branch").arg(tag)
                .arg(repo_url)
                .arg(dest_path)
        );
        
        // Execute commands
        let output = group.output()?;
        
        if !output.status.success() {
            return Err(RepositoryError::GitError(
                String::from_utf8_lossy(&output.stderr).into_owned()
            ));
        }
        
        Ok(())
    }

    fn git_fetch_checkout(&self, repo_url: &str, tag: &str, dest_path: &Path) -> Result<(), RepositoryError> {
        let mut group = CommandGroup::new();
        
        // Fetch and checkout
        group.command(
            Command::new("git")
                .arg("fetch")
                .arg("--tags")
                .current_dir(dest_path)
        );
        
        group.command(
            Command::new("git")
                .arg("checkout")
                .arg(tag)
                .current_dir(dest_path)
        );
        
        // Execute commands
        let output = group.output()?;
        
        if !output.status.success() {
            return Err(RepositoryError::GitError(
                String::from_utf8_lossy(&output.stderr).into_owned()
            ));
        }
        
        Ok(())
    }
}
```

### 2. Stage3 Fetcher Implementation

```rust
// crossdev-stage3/src/lib.rs
use std::path::Path;
use std::process::Command;
use thiserror::Error;
use log::{info, error};

#[derive(Debug, Error)]
pub enum Stage3Error {
    #[error("Failed to fetch stage3 list: {0}")]
    FetchError(String),
    
    #[error("Failed to parse stage3 metadata: {0}")]
    ParseError(String),
    
    #[error("Failed to download stage3 image: {0}")]
    DownloadError(String),
    
    #[error("Failed to verify stage3 image: {0}")]
    VerifyError(String),
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct Stage3Info {
    pub name: String,
    pub url: String,
    pub size: u64,
    pub date: String,
    pub arch: String,
    pub flavor: String,
}

pub struct Stage3Fetcher {
    config: PlatformConfig,
    cache_dir: String,
}

impl Stage3Fetcher {
    pub fn new(config: PlatformConfig, cache_dir: &str) -> Self {
        Self {
            config,
            cache_dir: cache_dir.to_string(),
        }
    }

    /// Fetch the latest stage3 image for the target architecture
    pub async fn fetch_latest(&self) -> Result<Stage3Info, Stage3Error> {
        // Get the list of available stage3 images
        let stage3_list = self.fetch_stage3_list().await?;
        
        // Find the latest image for our target
        let latest = self.find_latest_stage3(&stage3_list)?;
        
        // Download the image
        self.download_stage3(&latest).await?;
        
        // Verify the image
        self.verify_stage3(&latest).await?;
        
        Ok(latest)
    }

    async fn fetch_stage3_list(&self) -> Result<Vec<Stage3Info>, Stage3Error> {
        let base_url = format!(
            "https://distfiles.gentoo.org/releases/{}/autobuilds/",
            self.config.target.arch
        );
        
        let latest_url = format!("{}/latest-{}.txt", base_url, self.config.target.flavor);
        
        info!("Fetching stage3 list from: {}", latest_url);
        
        // Use curl or wget to fetch the list
        let output = Command::new("curl")
            .arg("-s")
            .arg("-f")
            .arg(&latest_url)
            .output()?;
        
        if !output.status.success() {
            return Err(Stage3Error::FetchError(
                String::from_utf8_lossy(&output.stderr).into_owned()
            ));
        }
        
        // Parse the stage3 list
        self.parse_stage3_list(&String::from_utf8_lossy(&output.stdout))
    }

    fn parse_stage3_list(&self, content: &str) -> Result<Vec<Stage3Info>, Stage3Error> {
        // Parse the stage3 list format
        // Example format:
        // # Wed Oct 18 01:00:01 UTC 2023
        // stage3-riscv64-openrc-20231018T010001Z.tar.xz 123456789 SHA256 abc123...
        
        let mut stage3_images = Vec::new();
        
        for line in content.lines() {
            // Skip comments and empty lines
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            
            // Parse stage3 info
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let name = parts[0].to_string();
                let size = parts[1].parse::<u64>().map_err(|e| 
                    Stage3Error::ParseError(format!("Failed to parse size: {}", e))
                )?;
                // Let's assume the third part is the hash for now
                
                // Extract arch and flavor from name
                if name.starts_with("stage3-") && name.contains(&self.config.target.flavor) {
                    stage3_images.push(Stage3Info {
                        name: name.clone(),
                        url: format!("https://distfiles.gentoo.org/releases/{}/autobuilds/{}",
                            self.config.target.arch, name),
                        size,
                        date: "unknown".to_string(), // Extract from filename
                        arch: self.config.target.arch.clone(),
                        flavor: self.config.target.flavor.clone(),
                    });
                }
            }
        }
        
        if stage3_images.is_empty() {
            return Err(Stage3Error::ParseError(
                "No matching stage3 images found".to_string()
            ));
        }
        
        Ok(stage3_images)
    }

    fn find_latest_stage3(&self, images: &[Stage3Info]) -> Result<Stage3Info, Stage3Error> {
        // Find the most recent image (highest timestamp in filename)
        images.iter()
            .max_by(|a, b| {
                // Extract timestamp from filename: stage3-arch-flavor-YYYYMMDDTHHMMSSZ.tar.xz
                let a_ts = extract_timestamp(&a.name);
                let b_ts = extract_timestamp(&b.name);
                a_ts.cmp(&b_ts)
            })
            .cloned()
            .ok_or_else(|| Stage3Error::ParseError("No stage3 images available".to_string()))
    }

    async fn download_stage3(&self, stage3: &Stage3Info) -> Result<(), Stage3Error> {
        let cache_path = Path::new(&self.cache_dir).join(&stage3.name);
        
        // Create cache directory if it doesn't exist
        std::fs::create_dir_all(&self.cache_dir)?;
        
        info!("Downloading stage3 image: {}", stage3.name);
        
        // Use curl to download
        let output = Command::new("curl")
            .arg("-L")
            .arg("-o").arg(&cache_path)
            .arg(&stage3.url)
            .output()?;
        
        if !output.status.success() {
            return Err(Stage3Error::DownloadError(
                String::from_utf8_lossy(&output.stderr).into_owned()
            ));
        }
        
        info!("Downloaded stage3 image to: {}", cache_path.display());
        
        Ok(())
    }

    async fn verify_stage3(&self, stage3: &Stage3Info) -> Result<(), Stage3Error> {
        let cache_path = Path::new(&self.cache_dir).join(&stage3.name);
        
        // Verify the image exists and has correct size
        let metadata = std::fs::metadata(&cache_path)?;
        if metadata.len() != stage3.size {
            return Err(Stage3Error::VerifyError(
                format!("Size mismatch: expected {}, got {}", stage3.size, metadata.len())
            ));
        }
        
        // Additional verification could be added here
        // (e.g., checksum verification, signature checking)
        
        info!("Stage3 image verified successfully");
        
        Ok(())
    }

    /// Extract stage3 image to target directory
    pub fn extract_stage3(&self, stage3: &Stage3Info, target_dir: &str) -> Result<(), Stage3Error> {
        let cache_path = Path::new(&self.cache_dir).join(&stage3.name);
        
        info!("Extracting stage3 image to: {}", target_dir);
        
        // Use tar to extract
        let output = Command::new("tar")
            .arg("--exclude").arg("dev/*")
            .arg("-xJpf").arg(&cache_path)
            .arg("-C").arg(target_dir)
            .output()?;
        
        if !output.status.success() {
            return Err(Stage3Error::IoError(
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    String::from_utf8_lossy(&output.stderr).into_owned()
                )
            ));
        }
        
        info!("Stage3 image extracted successfully");
        
        Ok(())
    }
}

fn extract_timestamp(filename: &str) -> u64 {
    // Extract timestamp from filename: stage3-arch-flavor-YYYYMMDDTHHMMSSZ.tar.xz
    // This is a simplified extraction - real implementation would need proper parsing
    let parts: Vec<&str> = filename.split('-').collect();
    if parts.len() >= 4 {
        // Try to parse the timestamp part
        if let Ok(ts) = parts[parts.len() - 2].replace("T", "").replace("Z", "").parse::<u64>() {
            return ts;
        }
    }
    0
}
```

## Updated Dependency Analysis

### Removed Dependencies

- `git2 = "0.17"` - No longer needed, using Git CLI instead

### Simplified Dependencies

```toml
[workspace.dependencies]
# Core (unchanged)
serde = { version = "1.0", features = ["derive"] }
thiserror = "1.0"
log = "0.4"
config = "0.13"
toml = "0.7"
clap = { version = "4.0", features = ["derive"] }
tokio = { version = "1.0", features = ["process", "fs"] }
env_logger = "0.10"

# Command execution (unchanged)
command-group = "1.0"

# HTTP (optional for stage3-fetcher)
reqwest = { version = "0.11", features = ["json"] }

# Testing (unchanged)
mockall = "0.11"
assert_cmd = "2.0"
predicates = "2.1"
```

### Crate-Specific Dependencies

#### crossdev-stage3
```toml
[dependencies]
crossdev-config = { path = "../crossdev-config" }
thiserror = "1.0"
log = "0.4"
command-group = "1.0"
tokio = { version = "1.0", features = ["process", "fs"] }

# Optional: Use reqwest for HTTP instead of curl
# reqwest = { version = "0.11", features = ["json"] }

[dev-dependencies]
mockall = "0.11"
```

#### crossdev-image (updated)
```toml
[dependencies]
crossdev-config = { path = "../crossdev-config" }
crossdev-stage3 = { path = "../crossdev-stage3" }
thiserror = "1.0"
log = "0.4"
command-group = "1.0"
tokio = { version = "1.0", features = ["process", "fs"] }

[dev-dependencies]
mockall = "0.11"
```

## Benefits of Simplified Approach

### 1. Reduced Complexity
- **Before**: Complex git2 integration
- **After**: Simple Git CLI commands
- **Impact**: Easier to implement and debug

### 2. Better Separation of Concerns
- **Before**: Mixed stage3 fetching logic
- **After**: Dedicated crate with clear API
- **Impact**: More maintainable and reusable

### 3. Improved Testability
- **Before**: Complex git2 mocking
- **After**: Simple command mocking
- **Impact**: Easier to test

### 4. Better Error Handling
- **Before**: Mixed error types
- **After**: Clear error types per crate
- **Impact**: Better error messages and handling

## Implementation Roadmap Update

### Phase 1: Foundation (2 weeks)
- [ ] Setup Rust workspace structure
- [ ] Implement configuration system (crossdev-config)
- [ ] Create stage3 fetcher (crossdev-stage3) ← **NEW**
- [ ] Create system utilities (crossdev-utils)
- [ ] Setup CI/CD pipeline

### Phase 2: Core Functionality (3 weeks)
- [ ] Implement cross-compilation core (crossdev-core)
- [ ] Create image building (crossdev-image) with Git CLI
- [ ] Basic CLI interface
- [ ] Integration tests

### Phase 3: CLI and Integration (2 weeks)
- [ ] Complete CLI interface
- [ ] Integration testing
- [ ] Documentation
- [ ] Performance optimization

## API Design for stage3-fetcher

### Public API

```rust
pub struct Stage3Fetcher {
    /// Create a new Stage3Fetcher
    pub fn new(config: PlatformConfig, cache_dir: &str) -> Self;
    
    /// Fetch the latest stage3 image
    pub async fn fetch_latest(&self) -> Result<Stage3Info, Stage3Error>;
    
    /// Extract stage3 image to target directory
    pub fn extract_stage3(&self, stage3: &Stage3Info, target_dir: &str) -> Result<(), Stage3Error>;
    
    /// Get cached stage3 images
    pub fn get_cached_images(&self) -> Result<Vec<Stage3Info>, Stage3Error>;
    
    /// Clear stage3 cache
    pub fn clear_cache(&self) -> Result<(), Stage3Error>;
}

pub struct Stage3Info {
    pub name: String,      // e.g., "stage3-riscv64-openrc-20231018T010001Z.tar.xz"
    pub url: String,       // Full download URL
    pub size: u64,         // Size in bytes
    pub date: String,      // Build date
    pub arch: String,      // Architecture (e.g., "riscv64")
    pub flavor: String,    // Flavor (e.g., "rv64_lp64d-openrc")
}
```

### Error Handling

```rust
pub enum Stage3Error {
    FetchError(String),    // Failed to fetch stage3 list
    ParseError(String),    // Failed to parse stage3 metadata
    DownloadError(String), // Failed to download stage3 image
    VerifyError(String),   // Failed to verify stage3 image
    IoError(std::io::Error), // IO errors
    // ... other variants as needed
}
```

## Testing Strategy for stage3-fetcher

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use mockall::predicate::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_stage3_list() {
        let config = PlatformConfig {
            target: TargetConfig {
                arch: "riscv64".to_string(),
                flavor: "rv64_lp64d-openrc".to_string(),
                // ... other fields
            },
            // ... other fields
        };
        
        let fetcher = Stage3Fetcher::new(config, "/tmp/cache");
        
        let test_data = r#"
# Wed Oct 18 01:00:01 UTC 2023
stage3-riscv64-openrc-20231018T010001Z.tar.xz 123456789 SHA256 abc123...
stage3-riscv64-openrc-20231017T010001Z.tar.xz 123456788 SHA256 def456...
"#;
        
        let result = fetcher.parse_stage3_list(test_data);
        assert!(result.is_ok());
        let images = result.unwrap();
        assert_eq!(images.len(), 2);
        assert!(images[0].name.contains("20231018"));
    }

    #[test]
    fn test_find_latest_stage3() {
        let config = PlatformConfig {
            target: TargetConfig {
                arch: "riscv64".to_string(),
                flavor: "rv64_lp64d-openrc".to_string(),
            },
            // ...
        };
        
        let fetcher = Stage3Fetcher::new(config, "/tmp/cache");
        
        let images = vec![
            Stage3Info {
                name: "stage3-riscv64-openrc-20231017T010001Z.tar.xz".to_string(),
                url: "http://example.com/1.tar.xz".to_string(),
                size: 100,
                date: "2023-10-17".to_string(),
                arch: "riscv64".to_string(),
                flavor: "rv64_lp64d-openrc".to_string(),
            },
            Stage3Info {
                name: "stage3-riscv64-openrc-20231018T010001Z.tar.xz".to_string(),
                url: "http://example.com/2.tar.xz".to_string(),
                size: 101,
                date: "2023-10-18".to_string(),
                arch: "riscv64".to_string(),
                flavor: "rv64_lp64d-openrc".to_string(),
            },
        ];
        
        let latest = fetcher.find_latest_stage3(&images).unwrap();
        assert!(latest.name.contains("20231018"));
    }
}
```

### Integration Tests

```rust
#[tokio::test]
async fn test_fetch_latest_stage3() {
    // This would test the actual fetching functionality
    // Would need network access or mock HTTP server
}
```

## Migration Path

### From Shell to Rust

1. **Initial Implementation**:
   - Implement stage3-fetcher in Rust
   - Keep using shell scripts for main functionality
   - Call Rust stage3-fetcher from shell scripts

2. **Gradual Transition**:
   - Replace shell stage3 fetching with Rust implementation
   - Verify functionality matches exactly
   - Performance benchmarking

3. **Full Transition**:
   - Move to full Rust implementation
   - Remove shell script dependencies
   - Final testing and validation

### Shell Script Integration

```bash
# In existing shell scripts, call Rust stage3-fetcher
STAGE3_INFO=$(crossdev-stage3 fetch-latest --arch riscv64 --flavor rv64_lp64d-openrc)
STAGE3_URL=$(echo "$STAGE3_INFO" | jq -r '.url')
STAGE3_NAME=$(echo "$STAGE3_INFO" | jq -r '.name')

# Extract stage3
crossdev-stage3 extract "$STAGE3_NAME" /path/to/stage
```

## Benefits Summary

### Simplified Git Approach
- ✅ **Easier Implementation**: Use familiar Git CLI
- ✅ **Better Debugging**: Git commands are visible and understandable
- ✅ **Reduced Dependencies**: No need for git2 crate
- ✅ **Compatibility**: Works with existing Git installations

### Dedicated Stage3 Fetcher
- ✅ **Clear Separation**: Stage3 logic in its own crate
- ✅ **Reusability**: Can be used by other tools
- ✅ **Testability**: Easier to test complex logic
- ✅ **Maintainability**: Clear API and error handling

### Overall Benefits
- ✅ **Faster Development**: Simpler approach gets to working code faster
- ✅ **Better Architecture**: Clear separation of concerns
- ✅ **Easier Testing**: Simpler components are easier to test
- ✅ **Improved Maintainability**: Well-organized code structure

## Conclusion

This simplified approach provides a more practical path to Rust implementation:

1. **Start with stage3-fetcher**: Extract complex logic first
2. **Use Git CLI**: Avoid git2 complexity
3. **Gradual transition**: Move functionality piece by piece
4. **Maintain compatibility**: Ensure same behavior as shell scripts

The result will be a more maintainable, testable, and performant implementation while keeping the transition manageable.