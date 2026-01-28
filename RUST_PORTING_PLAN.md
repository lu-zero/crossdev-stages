# Rust Porting Plan for crossdev-stages

## Analysis of Current Functionality

### 1. Core Functionality Analysis

#### cross-stage.sh Capabilities
- **Configuration Management**: Load platform-specific settings
- **Cross-compilation Setup**: Initialize crossdev environment
- **Stage Management**: Create/update Gentoo stages
- **Package Management**: Install packages in stages
- **System Utilities**: ldconfig management, bubblewrap execution

#### make-image.sh Capabilities
- **Source Management**: Git repository checkout and management
- **Build System**: Bootloader and kernel compilation
- **Image Assembly**: Filesystem creation and image generation
- **Configuration**: Platform-specific image layouts

### 2. Key Components to Port

#### Configuration System
- Platform configurations (TOML/JSON/YAML)
- Package lists management
- Environment variable handling

#### Cross-compilation Management
- crossdev initialization
- Portage configuration
- Package emergence
- Toolchain management

#### Image Building
- Source repository management
- Build process orchestration
- Filesystem creation
- Image generation with genimage

#### System Utilities
- Bubblewrap container execution
- ldconfig management
- File system operations

## Rust Workspace Design

### 1. Workspace Layout

```
crossdev-stages-rust/
├── Cargo.toml                  # Workspace root
├── crates/
│   ├── crossdev-config/        # Configuration management
│   ├── crossdev-core/          # Core cross-compilation logic
│   ├── crossdev-image/         # Image building
│   ├── crossdev-cli/           # CLI interface
│   └── crossdev-utils/         # System utilities
├── config/                     # Configuration files (ported)
├── docs/                       # Documentation
└── tests/                      # Integration tests
```

### 2. Crate Responsibilities

#### crossdev-config
- **Purpose**: Configuration management
- **Responsibilities**:
  - Load and validate configuration files
  - Manage platform-specific settings
  - Handle package lists
  - Environment variable management
- **Dependencies**:
  - `serde` for serialization
  - `config` crate for configuration
  - `thiserror` for error handling

#### crossdev-core
- **Purpose**: Cross-compilation management
- **Responsibilities**:
  - crossdev environment setup
  - Portage configuration management
  - Package emergence orchestration
  - Stage creation and management
- **Dependencies**:
  - `crossdev-config` for settings
  - `tokio` for async operations
  - `command-group` for command execution
  - `log` for logging

#### crossdev-image
- **Purpose**: Image building
- **Responsibilities**:
  - Source repository management
  - Build process orchestration
  - Filesystem creation
  - Image generation with genimage
- **Dependencies**:
  - `crossdev-config` for settings
  - `git2` for Git operations
  - `tempfile` for temporary files
  - `fs_extra` for filesystem operations

#### crossdev-utils
- **Purpose**: System utilities
- **Responsibilities**:
  - Bubblewrap container execution
  - ldconfig management
  - File system operations
  - Process management
- **Dependencies**:
  - `nix` for Unix-specific operations
  - `users` for user management
  - `libc` for system calls

#### crossdev-cli
- **Purpose**: Command line interface
- **Responsibilities**:
  - CLI argument parsing
  - Command dispatching
  - User interaction
  - Help and documentation
- **Dependencies**:
  - `clap` for CLI parsing
  - Other crates for functionality

## Detailed Implementation Plan

### Phase 1: Configuration System

#### 1. Configuration File Format
```rust
// config/platforms/riscv64-k1.toml
[target]
arch = "riscv64"
chost = "riscv64-unknown-linux-gnu"
flavor = "rv64_lp64d-openrc"
keyword = "riscv"

[compilation]
cflags = "-O3 -march=rv64gcv_zvl256b -pipe"
gcc_version = "16.0.0_p20251005"
profile = "default/linux/riscv/23.0/rv64/lp64d"

[repositories]
opensbi_repo = "https://github.com/cyyself/opensbi"
opensbi_tag = "k1-opensbi"
u_boot_repo = "https://gitee.com/bianbu-linux/uboot-2022.10.git"
# ... other repositories

[packages]
stage1_file = "stage1-packages.txt"
additional_file = "additional-packages.txt"

[image]
root_size = "5G"
boot_size = "500M"
genimage_config = "genimage-k1.cfg"
```

#### 2. Configuration Loading
```rust
// crossdev-config/src/lib.rs
use serde::Deserialize;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Deserialize)]
pub struct PlatformConfig {
    pub target: TargetConfig,
    pub compilation: CompilationConfig,
    pub repositories: RepositoryConfig,
    pub packages: PackageConfig,
    pub image: ImageConfig,
}

#[derive(Debug, Deserialize)]
pub struct TargetConfig {
    pub arch: String,
    pub chost: String,
    pub flavor: String,
    pub keyword: String,
}

// Implement loading from TOML/JSON/YAML
pub fn load_config<P: AsRef<Path>>(path: P) -> Result<PlatformConfig, ConfigError> {
    // Implementation using config crate
}
```

### Phase 2: Cross-compilation Core

#### 1. Crossdev Environment Setup
```rust
// crossdev-core/src/crossdev.rs
use std::process::Command;
use crate::config::PlatformConfig;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CrossdevError {
    #[error("Crossdev setup failed: {0}")]
    SetupFailed(String),
    #[error("Portage configuration failed: {0}")]
    PortageConfigFailed(String),
    // ... other error variants
}

pub struct CrossdevManager {
    config: PlatformConfig,
    root_path: String,
}

impl CrossdevManager {
    pub fn new(config: PlatformConfig, root_path: &str) -> Self {
        Self {
            config,
            root_path: root_path.to_string(),
        }
    }

    pub fn setup_environment(&self) -> Result<(), CrossdevError> {
        // Initialize crossdev
        self.init_crossdev()?;
        
        // Configure portage
        self.configure_portage()?;
        
        // Setup directories
        self.setup_directories()?;
        
        Ok(())
    }

    fn init_crossdev(&self) -> Result<(), CrossdevError> {
        let output = Command::new("crossdev")
            .arg(&self.config.target.chost)
            .arg("--init-target")
            .output()?;
        
        if !output.status.success() {
            return Err(CrossdevError::SetupFailed(
                String::from_utf8_lossy(&output.stderr).into_owned()
            ));
        }
        Ok(())
    }

    // Other methods: configure_portage, setup_directories, etc.
}
```

#### 2. Package Management
```rust
// crossdev-core/src/packages.rs
use std::process::Command;
use crate::config::PlatformConfig;

pub struct PackageManager {
    config: PlatformConfig,
    root_path: String,
}

impl PackageManager {
    pub fn new(config: PlatformConfig, root_path: &str) -> Self {
        Self {
            config,
            root_path: root_path.to_string(),
        }
    }

    pub fn install_stage1(&self) -> Result<(), PackageError> {
        // Load package lists
        let stage1_packages = self.load_package_list("stage1")?;
        
        // Install packages
        self.emerge_packages(&stage1_packages)?;
        
        Ok(())
    }

    fn emerge_packages(&self, packages: &[String]) -> Result<(), PackageError> {
        let mut command = Command::new(format!("{}-emerge", self.config.target.chost));
        
        for package in packages {
            command.arg(package);
        }
        
        // Execute and handle output
        // ...
    }
}
```

### Phase 3: Image Building

#### 1. Source Repository Management
```rust
// crossdev-image/src/repositories.rs
use git2::Repository;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("Git operation failed: {0}")]
    GitError(#[from] git2::Error),
    #[error("Repository not found: {0}")]
    NotFound(String),
    // ... other variants
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
            let repo = Repository::open(&dest_path)?;
            self.update_repository(&repo, tag)?;
        } else {
            // Clone new repository
            Repository::clone(repo_url, &dest_path)?;
            let repo = Repository::open(&dest_path)?;
            self.checkout_tag(&repo, tag)?;
        }
        
        Ok(())
    }

    // Helper methods: update_repository, checkout_tag, etc.
}
```

#### 2. Build Process Orchestration
```rust
// crossdev-image/src/builder.rs
use std::process::Command;
use crate::repositories::RepositoryManager;

pub struct ImageBuilder {
    build_dir: String,
    stage_dir: String,
    config: PlatformConfig,
}

impl ImageBuilder {
    pub fn new(build_dir: &str, stage_dir: &str, config: PlatformConfig) -> Self {
        Self {
            build_dir: build_dir.to_string(),
            stage_dir: stage_dir.to_string(),
            config,
        }
    }

    pub fn build_all(&self) -> Result<(), BuildError> {
        // Checkout sources
        let repo_manager = RepositoryManager::new(&self.build_dir);
        repo_manager.checkout_all(&self.config.repositories)?;
        
        // Build bootloader
        self.build_bootloader()?;
        
        // Build kernel
        self.build_kernel()?;
        
        // Copy to root
        self.copy_to_root()?;
        
        // Create boot
        self.copy_to_boot()?;
        
        // Generate image
        self.generate_image()?;
        
        Ok(())
    }

    fn build_bootloader(&self) -> Result<(), BuildError> {
        // Build OpenSBI
        self.build_opensbi()?;
        
        // Build U-Boot
        self.build_uboot()?;
        
        Ok(())
    }

    fn build_opensbi(&self) -> Result<(), BuildError> {
        let opensbi_dir = Path::new(&self.build_dir).join("opensbi");
        
        let output = Command::new("make")
            .current_dir(opensbi_dir)
            .arg("PLATFORM=generic")
            .arg("PLATFORM_DEFCONFIG=defconfig")
            .arg(format!("-j{}", num_cpus::get()))
            .arg("LLVM=1")
            .output()?;
        
        // Handle output and errors
        // ...
    }

    // Other build methods
}
```

### Phase 4: System Utilities

#### 1. Bubblewrap Container Execution
```rust
// crossdev-utils/src/bubblewrap.rs
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BubblewrapError {
    #[error("Bubblewrap execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Container setup failed: {0}")]
    SetupFailed(String),
}

pub struct BubblewrapRunner {
    chroot_dir: String,
}

impl BubblewrapRunner {
    pub fn new(chroot_dir: &str) -> Self {
        Self {
            chroot_dir: chroot_dir.to_string(),
        }
    }

    pub fn run(&self, command: &str, args: &[&str]) -> Result<(), BubblewrapError> {
        let mut bwrap_cmd = Command::new("sudo");
        bwrap_cmd.arg("bwrap");
        
        // Add bubblewrap arguments
        bwrap_cmd.arg("--bind").arg(&self.chroot_dir).arg("/");
        bwrap_cmd.arg("--dev-bind").arg("/dev").arg("dev");
        bwrap_cmd.arg("--proc").arg("/proc");
        bwrap_cmd.arg("--bind").arg("/sys").arg("sys");
        bwrap_cmd.arg("--ro-bind").arg("/etc/resolv.conf").arg("etc/resolv.conf");
        bwrap_cmd.arg("--hostname").arg("gentoo");
        bwrap_cmd.arg("--clearenv");
        
        // Add environment variables
        bwrap_cmd.arg("--setenv").arg("TERM").arg("xterm");
        bwrap_cmd.arg("--setenv").arg("HOME").arg("/root");
        bwrap_cmd.arg("--unshare-uts");
        
        // Add the command to run
        bwrap_cmd.arg(command);
        for arg in args {
            bwrap_cmd.arg(arg);
        }
        
        let output = bwrap_cmd.output()?;
        
        if !output.status.success() {
            return Err(BubblewrapError::ExecutionFailed(
                String::from_utf8_lossy(&output.stderr).into_owned()
            ));
        }
        
        Ok(())
    }
}
```

#### 2. ldconfig Management
```rust
// crossdev-utils/src/ldconfig.rs
use std::process::Command;

pub struct LdconfigManager {
    stage_dir: String,
}

impl LdconfigManager {
    pub fn new(stage_dir: &str) -> Self {
        Self {
            stage_dir: stage_dir.to_string(),
        }
    }

    pub fn update(&self) -> Result<(), LdconfigError> {
        let output = Command::new("ldconfig")
            .arg("-v")
            .arg("-C").arg("/etc/ld.so.cache")
            .arg("-r").arg(&self.stage_dir)
            .output()?;
        
        if !output.status.success() {
            return Err(LdconfigError::UpdateFailed(
                String::from_utf8_lossy(&output.stderr).into_owned()
            ));
        }
        
        Ok(())
    }
}
```

### Phase 5: CLI Interface

#### 1. Main CLI Structure
```rust
// crossdev-cli/src/main.rs
use clap::{Arg, Command, Subcommand};
use crossdev_config::PlatformConfig;
use crossdev_core::CrossdevManager;
use crossdev_image::ImageBuilder;
use std::path::PathBuf;

#[derive(Subcommand, Debug)]
enum CrossdevCommand {
    /// Setup crossdev environment
    Prepare,
    
    /// Create a new stage1
    Make {
        /// Stage directory
        stage_dir: PathBuf,
    },
    
    /// Update a pre-existing stage3
    Update {
        /// Stage directory
        stage_dir: PathBuf,
    },
    
    /// Install additional packages
    InstallMore {
        /// Stage directory
        stage_dir: PathBuf,
    },
    
    /// Build bootable image
    BuildImage {
        /// Build directory
        build_dir: PathBuf,
        /// Stage directory
        stage_dir: PathBuf,
    },
}

#[derive(Debug)]
struct AppArgs {
    /// Configuration file
    config_file: Option<PathBuf>,
    /// Platform name
    platform: Option<String>,
    /// Verbose output
    verbose: bool,
    /// Command to execute
    command: CrossdevCommand,
}

fn parse_args() -> AppArgs {
    let matches = Command::new("crossdev-stages")
        .version("1.0")
        .about("Gentoo cross-compilation stage builder")
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .value_name("FILE")
                .help("Use alternative configuration file")
                .takes_value(true),
        )
        .arg(
            Arg::new("platform")
                .short('p')
                .long("platform")
                .value_name("NAME")
                .help("Use specific platform configuration")
                .takes_value(true),
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .help("Verbose output"),
        )
        .subcommand(
            Command::new("prepare")
                .about("Setup crossdev environment"),
        )
        .subcommand(
            Command::new("make")
                .about("Create a new stage1")
                .arg(Arg::new("stage_dir").required(true)),
        )
        // ... other subcommands
        .get_matches();

    // Parse arguments and return AppArgs
    // ...
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args();
    
    // Load configuration
    let config = load_configuration(&args.config_file, &args.platform)?;
    
    // Execute command
    match args.command {
        CrossdevCommand::Prepare => {
            let manager = CrossdevManager::new(config, "/usr");
            manager.setup_environment()?;
        }
        CrossdevCommand::Make { stage_dir } => {
            let manager = CrossdevManager::new(config, "/usr");
            manager.create_stage1(stage_dir.to_str().unwrap())?;
        }
        // ... other commands
    }
    
    Ok(())
}
```

## Error Handling Strategy

### 1. Error Types

```rust
// crossdev-utils/src/error.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CrossdevError {
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),
    
    #[error("Cross-compilation error: {0}")]
    Crossdev(#[from] CrossdevError),
    
    #[error("Image building error: {0}")]
    Image(#[from] BuildError),
    
    #[error("System utility error: {0}")]
    Utility(#[from] UtilityError),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Command execution failed: {0}")]
    Command(String),
}

// Define specific error types for each crate
```

### 2. Result Type Alias

```rust
// Common result type for the application
pub type Result<T> = std::result::Result<T, CrossdevError>;
```

### 3. Error Handling Pattern

```rust
fn example_function() -> Result<()> {
    // Operation that might fail
    let result = some_fallible_operation()?;
    
    // Another operation
    another_operation(result)?;
    
    Ok(())
}

fn some_fallible_operation() -> Result<String> {
    // Try something
    let file = std::fs::File::open("config.toml")
        .map_err(|e| CrossdevError::Io(e))?;
    
    // Process file
    // ...
    
    Ok("result".to_string())
}
```

## Testing Strategy

### 1. Unit Testing

```rust
// tests/unit/config_tests.rs
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_load_config() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("test.toml");
        
        // Create test config file
        std::fs::write(
            &config_path,
            r#"
                [target]
                arch = "test"
                chost = "test-unknown-linux-gnu"
            "#
        ).unwrap();
        
        // Test loading
        let config = load_config(&config_path).unwrap();
        assert_eq!(config.target.arch, "test");
    }

    #[test]
    fn test_invalid_config() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("invalid.toml");
        
        // Create invalid config
        std::fs::write(&config_path, "invalid toml [[[").unwrap();
        
        // Test error handling
        let result = load_config(&config_path);
        assert!(result.is_err());
    }
}
```

### 2. Integration Testing

```rust
// tests/integration/crossdev_tests.rs
#[test]
fn test_crossdev_setup() {
    // This would be a more complex integration test
    // Using test containers or mock systems
}
```

### 3. Mocking External Commands

```rust
// tests/mocks/command_mock.rs
pub struct CommandMock {
    pub expected_commands: Vec<(String, Vec<String>)>,
    pub responses: Vec<(i32, String, String)>,  // exit_code, stdout, stderr
}

impl CommandMock {
    pub fn new() -> Self {
        Self {
            expected_commands: Vec::new(),
            responses: Vec::new(),
        }
    }

    pub fn expect(&mut self, command: &str, args: Vec<&str>) -> &mut Self {
        self.expected_commands.push((command.to_string(), args.iter().map(|s| s.to_string()).collect()));
        self
    }

    pub fn respond_with(&mut self, exit_code: i32, stdout: &str, stderr: &str) -> &mut Self {
        self.responses.push((exit_code, stdout.to_string(), stderr.to_string()));
        self
    }
}
```

## Implementation Roadmap

### Phase 1: Foundation (2-3 weeks)
- [ ] Setup Rust workspace structure
- [ ] Implement configuration system (crossdev-config)
- [ ] Create basic error handling framework
- [ ] Setup CI/CD pipeline
- [ ] Write initial unit tests

### Phase 2: Core Functionality (3-4 weeks)
- [ ] Implement cross-compilation core (crossdev-core)
- [ ] Create system utilities (crossdev-utils)
- [ ] Basic CLI interface
- [ ] Integration tests for core functionality

### Phase 3: Image Building (2-3 weeks)
- [ ] Implement image building (crossdev-image)
- [ ] Source repository management
- [ ] Build process orchestration
- [ ] Image generation

### Phase 4: CLI and Integration (2 weeks)
- [ ] Complete CLI interface
- [ ] Integration testing
- [ ] Documentation
- [ ] Performance optimization

### Phase 5: Testing and Deployment (1-2 weeks)
- [ ] Comprehensive test suite
- [ ] User testing and feedback
- [ ] Bug fixes and polish
- [ ] Release preparation

## Dependency Management

### Cargo.toml (Workspace)

```toml
[workspace]
members = [
    "crates/crossdev-config",
    "crates/crossdev-core",
    "crates/crossdev-image",
    "crates/crossdev-utils",
    "crates/crossdev-cli",
]
resolver = "2"

[workspace.dependencies]
serde = { version = "1.0", features = ["derive"] }
thiserror = "1.0"
log = "0.4"
tokio = { version = "1.0", features = ["full"] }
clap = { version = "4.0", features = ["derive"] }
config = "0.13"
git2 = "0.17"
tempfile = "3.3"
fs_extra = "1.2"
nix = "0.26"
users = "0.11"
libc = "0.2"
command-group = "1.0"
```

### Crate-Specific Dependencies

#### crossdev-config
```toml
[dependencies]
serde = { workspace = true, features = ["derive"] }
thiserror = { workspace = true }
config = { workspace = true }
toml = "0.7"
```

#### crossdev-core
```toml
[dependencies]
crossdev-config = { path = "../crossdev-config" }
thiserror = { workspace = true }
log = { workspace = true }
tokio = { workspace = true, features = ["process"] }
command-group = { workspace = true }
```

## Migration Strategy

### 1. Parallel Implementation
- Keep shell scripts working during Rust development
- Use Rust implementation for new features first
- Gradually replace shell script functionality

### 2. Feature Parity
- Ensure Rust implementation has same features as shell scripts
- Maintain same CLI interface for compatibility
- Preserve all configuration options

### 3. Testing and Validation
- Compare outputs between shell and Rust implementations
- Performance benchmarking
- User acceptance testing

### 4. Deployment
- Initial deployment alongside shell scripts
- Gradual transition period
- Final removal of shell scripts when Rust is stable

## Benefits of Rust Implementation

### 1. Performance
- Faster execution than shell scripts
- Better resource utilization
- Parallel processing capabilities

### 2. Safety
- Memory safety guarantees
- Better error handling
- Type safety

### 3. Maintainability
- Clearer code organization
- Better documentation
- Easier to extend

### 4. Cross-platform
- Easier to support multiple platforms
- Consistent behavior across systems
- Better dependency management

### 5. Tooling
- Better IDE support
- Advanced refactoring tools
- Comprehensive testing frameworks

## Challenges and Mitigations

### 1. Shell Command Dependencies
**Challenge**: Shell scripts rely on many external commands
**Mitigation**: Use Rust crates where possible, fall back to command execution

### 2. Complex Build Processes
**Challenge**: Build processes have many dependencies and edge cases
**Mitigation**: Gradual implementation with thorough testing

### 3. Configuration Compatibility
**Challenge**: Ensure configuration files work with both implementations
**Mitigation**: Use same configuration format, validate compatibility

### 4. User Transition
**Challenge**: Users accustomed to shell scripts
**Mitigation**: Maintain same CLI interface, provide migration guides

## Conclusion

This comprehensive plan outlines a structured approach to porting the crossdev-stages functionality from shell scripts to Rust. The design emphasizes:

1. **Separation of Concerns**: Clear crate boundaries with single responsibilities
2. **Modular Architecture**: Easy to extend and maintain
3. **Error Handling**: Comprehensive error management
4. **Testing**: Unit and integration testing strategy
5. **Migration**: Gradual transition with compatibility

The Rust implementation will provide better performance, safety, and maintainability while preserving all existing functionality and user workflows.