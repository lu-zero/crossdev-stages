# Rust Developer Guide

## Getting Started

### Prerequisites

- Rust 1.70+ (install via [rustup](https://rustup.rs/))
- Git
- Curl (for stage3 fetching)
- Tar (for stage3 extraction)
- Basic build tools

### Setup

```bash
# Clone the repository (if you haven't already)
git clone <repository-url>
cd crossdev-stages

# Navigate to Rust workspace
cd crossdev-stages-rust

# Build the project
cargo build

# Run tests
cargo test --all
```

## Project Structure

```
crossdev-stages-rust/
├── Cargo.toml                  # Workspace configuration
├── crates/                      # Rust crates
│   ├── crossdev-cli/           # CLI interface
│   ├── crossdev-config/        # Configuration management
│   ├── crossdev-core/          # Cross-compilation logic
│   ├── crossdev-image/         # Image building
│   ├── crossdev-stage3/        # Stage3 fetching
│   └── crossdev-utils/         # System utilities
└── config/                     # Configuration files
    └── platforms/
        └── riscv64-k1.toml     # Platform configuration
```

## Crate Responsibilities

### crossdev-config
**Purpose**: Load and manage platform configurations

**Key Types**:
- `PlatformConfig` - Main configuration structure
- `TargetConfig` - Target architecture settings
- `CompilationConfig` - Compilation flags and settings
- `RepositoryConfig` - Source repository URLs
- `PackageConfig` - Package list references
- `ImageConfig` - Image generation settings

**Usage**:
```rust
use crossdev_config::PlatformConfig;

// Load from file
let config = PlatformConfig::load_from_file("config.toml")?;

// Or create programmatically
let config = PlatformConfig {
    target: crossdev_config::TargetConfig {
        arch: "riscv64".to_string(),
        chost: "riscv64-unknown-linux-gnu".to_string(),
        flavor: "rv64_lp64d-openrc".to_string(),
        keyword: "riscv".to_string(),
    },
    // ... other fields
};
```

### crossdev-stage3
**Purpose**: Fetch, cache, and extract Gentoo stage3 images

**Key Types**:
- `Stage3Fetcher` - Main fetcher class
- `Stage3Info` - Stage3 image metadata

**Usage**:
```rust
use crossdev_config::PlatformConfig;
use crossdev_stage3::Stage3Fetcher;

// Create fetcher
let config = PlatformConfig::load_from_file("config.toml")?;
let fetcher = Stage3Fetcher::new(config, "/tmp/cache");

// Fetch latest stage3
let stage3 = fetcher.fetch_latest()?;

// Extract to directory
fetcher.extract_stage3(&stage3, "/path/to/extract")?;
```

### crossdev-cli
**Purpose**: Command line interface

**Key Features**:
- Clap-based argument parsing
- Subcommand support
- Help system
- Logging

**Usage**:
```rust
use clap::{Arg, Command};
use crossdev_config::PlatformConfig;

// Parse arguments
let matches = Command::new("crossdev-stages")
    .version("0.1.0")
    .about("Gentoo cross-compilation stage builder")
    .subcommand(
        Command::new("fetch-stage3")
            .about("Fetch latest stage3 image")
            .arg(Arg::new("config").short('c').long("config").value_name("FILE"))
            // ... more args
    )
    .get_matches();

// Load configuration
let config = if let Some(config_path) = matches.get_one::<String>("config") {
    PlatformConfig::load_from_file(config_path)?
} else {
    // Create from CLI args or defaults
};
```

## Coding Standards

### Error Handling

Use `thiserror` for error types:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MyError {
    #[error("Operation failed: {0}")]
    OperationFailed(String),
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}
```

Use `?` operator for error propagation:

```rust
pub fn my_function() -> Result<(), MyError> {
    let file = std::fs::File::open("config.toml")?;
    // ... rest of function
    Ok(())
}
```

### Logging

Use `log` crate with `env_logger`:

```rust
use log::{info, error, warn, debug, trace};

fn my_function() -> Result<(), MyError> {
    info!("Starting operation");
    debug!("Debug information: {}", value);
    
    if let Err(e) = some_operation() {
        error!("Operation failed: {}", e);
        return Err(e);
    }
    
    Ok(())
}
```

Initialize logger in main:

```rust
env_logger::Builder::from_default_env()
    .filter_level(LevelFilter::Info)
    .init();
```

### Testing

Write unit tests in `tests` module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    
    #[test]
    fn test_something() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("test.txt");
        
        // Test code here
        assert!(true);
    }
}
```

Use `tempfile` for temporary files/directories:

```rust
use tempfile::tempdir;

#[test]
fn test_file_operations() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    
    std::fs::write(&file_path, "test content").unwrap();
    
    let content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "test content");
}
```

## Common Patterns

### Command Execution

Use `std::process::Command`:

```rust
use std::process::Command;

fn run_command(command: &str, args: &[&str]) -> Result<String, MyError> {
    let output = Command::new(command)
        .args(args)
        .output()
        .map_err(|e| MyError::CommandFailed(e.to_string()))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MyError::CommandFailed(stderr.into_owned()));
    }
    
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
```

### File Operations

```rust
use std::fs;

// Read file
fn read_file(path: &str) -> Result<String, MyError> {
    let content = fs::read_to_string(path)?;
    Ok(content)
}

// Write file
fn write_file(path: &str, content: &str) -> Result<(), MyError> {
    fs::write(path, content)?;
    Ok(())
}

// Check if file exists
fn file_exists(path: &str) -> bool {
    Path::new(path).exists()
}
```

### Configuration Loading

```rust
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct MyConfig {
    pub setting1: String,
    pub setting2: u32,
}

impl MyConfig {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        
        if !path.exists() {
            return Err(ConfigError::NotFound(path.to_string_lossy().into_owned()));
        }
        
        let content = std::fs::read_to_string(path)?;
        let config: MyConfig = toml::from_str(&content)?;
        
        Ok(config)
    }
}
```

## Development Workflow

### Making Changes

1. **Pick a task** from the checklist
2. **Create a branch**
   ```bash
   git checkout -b feature/my-feature
   ```
3. **Implement the feature**
4. **Add tests**
5. **Run tests**
   ```bash
   cargo test --all
   ```
6. **Fix warnings**
   ```bash
   cargo clippy
   cargo fix
   ```
7. **Commit changes**
   ```bash
   git commit -m "feat: implement my feature"
   
   Generated by Mistral Vibe.
   Co-Authored-By: Mistral Vibe <vibe@mistral.ai>
   ```
8. **Push and create PR**

### Running Specific Tests

```bash
# Run all tests
cargo test --all

# Run tests for specific crate
cargo test -p crossdev-config
cargo test -p crossdev-stage3

# Run specific test
cargo test -p crossdev-config test_load_valid_config
```

### Building for Release

```bash
# Build release version
cargo build --release

# Run release version
./target/release/crossdev-stages --help
```

## Debugging Tips

### Common Issues

1. **Missing dependencies**
   ```bash
   cargo update
   cargo build
   ```

2. **Compilation errors**
   - Check error messages carefully
   - Use `cargo check` for faster feedback
   - Use `cargo clippy` for linting

3. **Test failures**
   - Run specific test to isolate issue
   - Add debug output with `println!`
   - Check test assumptions

4. **Runtime errors**
   - Enable verbose logging with `RUST_LOG=debug`
   - Check error types and messages
   - Add more detailed error context

### Logging Levels

```bash
# No logging (default)
cargo run

# Info level (default)
RUST_LOG=info cargo run

# Debug level
RUST_LOG=debug cargo run

# Trace level (very verbose)
RUST_LOG=trace cargo run

# Specific crate
RUST_LOG=crossdev_stage3=debug cargo run
```

## Useful Commands

```bash
# Build
cargo build

# Build release
cargo build --release

# Run
cargo run -- --help

# Test
cargo test --all

# Check for errors without building
cargo check

# Linting
cargo clippy

# Auto-fix warnings
cargo fix

# Update dependencies
cargo update

# Generate documentation
cargo doc --open

# Format code
cargo fmt
```

## Architecture Decisions

### Why Rust?

1. **Performance**: Faster than shell scripts
2. **Safety**: Memory safety, type safety
3. **Maintainability**: Better code organization
4. **Tooling**: Excellent IDE support
5. **Testing**: Comprehensive testing framework

### Why TOML for Configuration?

1. **Readable**: Human-friendly format
2. **Structured**: Clear hierarchy
3. **Type-safe**: Easy to parse with serde
4. **Versionable**: Good for version control

### Why Clap for CLI?

1. **Powerful**: Flexible argument parsing
2. **Derive**: Easy to use with macros
3. **Help**: Automatic help generation
4. **Subcommands**: Built-in support

## Contributing

### Code Reviews

- Follow Rust naming conventions
- Write clear commit messages
- Add tests for new functionality
- Document public APIs
- Keep functions focused and small

### Documentation

- Add Rustdoc comments for public items
- Document examples in code
- Update README for user-facing changes
- Keep CHANGELOG updated

### Testing

- Aim for high test coverage
- Test edge cases
- Test error scenarios
- Use property-based testing where appropriate

## Resources

- [Rust Documentation](https://doc.rust-lang.org/stable/book/)
- [Clap Documentation](https://docs.rs/clap/latest/clap/)
- [Serde Documentation](https://serde.rs/)
- [Thiserror Documentation](https://docs.rs/thiserror/latest/thiserror/)
- [Log Documentation](https://docs.rs/log/latest/log/)

## Support

For questions or issues:

1. Check existing documentation
2. Search GitHub issues
3. Create a new issue with:
   - Clear description
   - Steps to reproduce
   - Expected vs actual behavior
   - Rust version
   - Operating system
