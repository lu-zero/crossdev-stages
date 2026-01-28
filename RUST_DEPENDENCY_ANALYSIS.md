# Rust Dependency Analysis for crossdev-stages

## Dependency Philosophy

### Core Principles

1. **Minimalism**: Use only essential dependencies
2. **Stability**: Prefer mature, well-maintained crates
3. **Compatibility**: Ensure cross-platform support
4. **Performance**: Avoid unnecessary overhead
5. **Maintainability**: Prefer crates with good documentation

## Dependency Categories

### 1. Essential Dependencies

These are critical for core functionality:

```toml
# Configuration
serde = { version = "1.0", features = ["derive"] }  # Serialization
config = "0.13"  # Configuration file loading
toml = "0.7"     # TOML parsing

# Error Handling
thiserror = "1.0"  # Better error types
anyhow = "1.0"     # Convenient error handling

# CLI
clap = { version = "4.0", features = ["derive"] }  # Command line parsing

# Async Runtime
tokio = { version = "1.0", features = ["process", "fs"] }  # Async I/O

# Git Operations
git2 = "0.17"  # Git repository management

# Logging
log = "0.4"      # Logging facade
env_logger = "0.10"  # Simple logger
```

### 2. Optional Dependencies

These provide additional functionality but aren't essential:

```toml
# Filesystem Utilities
fs_extra = "1.2"  # Additional filesystem operations

# Temporary Files
tempfile = "3.3"  # Temporary file/directory management

# Unix-specific Operations
nix = "0.26"      # Unix system calls
users = "0.11"    # User management

# Command Execution
command-group = "1.0"  # Command grouping
duct = "0.13"     # Better command execution

# Configuration Alternatives
serde_json = "1.0"  # JSON support
serde_yaml = "0.9"  # YAML support
```

### 3. Development Dependencies

Only needed for testing and development:

```toml
# Testing
mockall = "0.11"  # Mocking framework
assert_cmd = "2.0"  # Command assertion testing
predicates = "2.1"  # Predicates for testing

# Benchmarking
criterion = "0.4"  # Performance benchmarking

# Documentation
doc-comment = "0.3"  # Documentation tools
```

## Dependency Analysis by Crate

### crossdev-config

**Essential**:
- `serde` + `derive` - For configuration serialization
- `config` - Configuration file loading
- `toml` - TOML parsing
- `thiserror` - Error handling

**Optional**:
- `serde_json` - JSON support
- `serde_yaml` - YAML support

```toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }
config = "0.13"
toml = "0.7"
thiserror = "1.0"

[dev-dependencies]
mockall = "0.11"
```

### crossdev-core

**Essential**:
- `crossdev-config` - Configuration
- `thiserror` - Error handling
- `log` - Logging
- `tokio` - Async operations
- `command-group` - Command execution

**Optional**:
- `duct` - Better command execution

```toml
[dependencies]
crossdev-config = { path = "../crossdev-config" }
thiserror = "1.0"
log = "0.4"
tokio = { version = "1.0", features = ["process", "fs"] }
command-group = "1.0"

[dev-dependencies]
mockall = "0.11"
assert_cmd = "2.0"
```

### crossdev-image

**Essential**:
- `crossdev-config` - Configuration
- `git2` - Git operations
- `thiserror` - Error handling
- `log` - Logging
- `tokio` - Async operations

**Optional**:
- `fs_extra` - Filesystem operations
- `tempfile` - Temporary files

```toml
[dependencies]
crossdev-config = { path = "../crossdev-config" }
git2 = "0.17"
thiserror = "1.0"
log = "0.4"
tokio = { version = "1.0", features = ["process", "fs"] }

[dev-dependencies]
mockall = "0.11"
```

### crossdev-utils

**Essential**:
- `thiserror` - Error handling
- `log` - Logging
- `nix` - Unix system calls
- `users` - User management

**Optional**:
- `libc` - Direct system calls
- `duct` - Command execution

```toml
[dependencies]
thiserror = "1.0"
log = "0.4"
nix = "0.26"
users = "0.11"

[dev-dependencies]
mockall = "0.11"
```

### crossdev-cli

**Essential**:
- `clap` - CLI parsing
- `crossdev-config` - Configuration
- `crossdev-core` - Core functionality
- `crossdev-image` - Image building
- `crossdev-utils` - Utilities
- `thiserror` - Error handling
- `log` - Logging

```toml
[dependencies]
clap = { version = "4.0", features = ["derive"] }
crossdev-config = { path = "../crossdev-config" }
crossdev-core = { path = "../crossdev-core" }
crossdev-image = { path = "../crossdev-image" }
crossdev-utils = { path = "../crossdev-utils" }
thiserror = "1.0"
log = "0.4"
env_logger = "0.10"

[dev-dependencies]
assert_cmd = "2.0"
predicates = "2.1"
```

## Dependency Optimization

### Reducing Dependency Bloat

1. **Avoid Feature Creep**: Only enable necessary features
   ```toml
   # Good: Only enable needed features
   tokio = { version = "1.0", features = ["process", "fs"] }
   
   # Bad: Enable all features
   tokio = { version = "1.0", features = ["full"] }
   ```

2. **Prefer Lightweight Alternatives**:
   ```toml
   # Prefer command-group over duct for simpler needs
   command-group = "1.0"  # Lighter weight
   
   # Only use duct if advanced features needed
   # duct = "0.13"
   ```

3. **Minimize Optional Dependencies**:
   - Only include what's actually needed
   - Use feature flags for optional functionality

### Dependency Tree Analysis

```
crossdev-cli
├── clap
├── crossdev-config
│   ├── serde (derive)
│   ├── config
│   ├── toml
│   └── thiserror
├── crossdev-core
│   ├── crossdev-config
│   ├── thiserror
│   ├── log
│   ├── tokio (process, fs)
│   └── command-group
├── crossdev-image
│   ├── crossdev-config
│   ├── git2
│   ├── thiserror
│   ├── log
│   └── tokio (process, fs)
└── crossdev-utils
    ├── thiserror
    ├── log
    ├── nix
    └── users
```

## Alternative Approaches

### 1. Command Execution

**Option A**: `command-group` (Recommended)
- Pros: Lightweight, simple API
- Cons: Less feature-rich

**Option B**: `duct`
- Pros: More features, better error handling
- Cons: Heavier dependency

**Decision**: Start with `command-group`, switch to `duct` if needed

### 2. Configuration Format

**Option A**: TOML (Recommended)
- Pros: Simple, human-readable, good for config files
- Cons: Less flexible than JSON

**Option B**: JSON
- Pros: Universal, good for APIs
- Cons: More verbose for config files

**Option C**: YAML
- Pros: Human-friendly, supports comments
- Cons: Complex parsing, security concerns

**Decision**: Primary TOML, optional JSON/YAML support

### 3. Async Runtime

**Option A**: `tokio` (Recommended)
- Pros: Mature, widely used, good ecosystem
- Cons: Larger runtime

**Option B**: `async-std`
- Pros: Smaller, simpler
- Cons: Less mature ecosystem

**Decision**: Use `tokio` for better ecosystem support

## Dependency Version Strategy

### Version Pinning

1. **Use Exact Versions in Workspace**:
   ```toml
   [workspace.dependencies]
   serde = { version = "1.0.152", features = ["derive"] }
   thiserror = "1.0.38"
   ```

2. **Allow Patch Updates in Crates**:
   ```toml
   [dependencies]
   serde = { workspace = true }
   thiserror = { workspace = true }
   ```

3. **Regular Dependency Updates**:
   - Monthly dependency review
   - Security updates immediately
   - Major version updates carefully

### Dependency Management Tools

1. **cargo-update**: Keep dependencies updated
   ```bash
   cargo install cargo-update
   cargo update
   ```

2. **cargo-audit**: Security auditing
   ```bash
   cargo install cargo-audit
   cargo audit
   ```

3. **cargo-deny**: License and security checking
   ```bash
   cargo install cargo-deny
   cargo deny check
   ```

## Performance Considerations

### 1. Compile Time Optimization

```toml
[profile.release]
opt-level = 3
lto = true
codegen-units = 1
panic = "abort"
```

### 2. Runtime Optimization

- Use `tokio` for I/O-bound operations
- Avoid unnecessary allocations
- Use efficient data structures
- Minimize external command execution

### 3. Memory Usage

- Prefer stack allocation where possible
- Use `Box` for large data structures
- Avoid unnecessary cloning
- Use `Cow` for string handling

## Cross-platform Considerations

### 1. Unix-specific Dependencies

```toml
# Unix-only crates
nix = "0.26"
users = "0.11"
```

**Mitigation**: Use conditional compilation
```rust
#[cfg(unix)]
use nix::unistd::*;
```

### 2. Windows Compatibility

**Challenges**:
- Unix-specific system calls
- Different filesystem behavior
- Process management differences

**Solutions**:
- Use conditional compilation
- Provide Windows alternatives where possible
- Document platform limitations

## Testing Dependencies

### Unit Testing

```toml
[dev-dependencies]
mockall = "0.11"  # Mocking framework
assert_cmd = "2.0"  # Command testing
predicates = "2.1"  # Assertion helpers
```

### Integration Testing

```toml
[dev-dependencies]
tempfile = "3.3"  # Temporary files/dirs
fs_extra = "1.2"  # Filesystem operations
```

### Benchmarking

```toml
[dev-dependencies]
criterion = "0.4"  # Performance benchmarking
```

## Dependency Documentation

### Recommended Documentation Approach

1. **Document Dependency Purpose**:
   ```rust
   /// Uses `serde` for configuration serialization
   /// Uses `config` crate for configuration file loading
   /// Uses `toml` for TOML parsing
   ```

2. **Document Alternative Approaches**:
   ```rust
   /// Alternative: Could use `serde_json` for JSON config files
   /// Alternative: Could use `duct` for more advanced command execution
   ```

3. **Document Version Requirements**:
   ```rust
   /// Requires `serde` 1.0+ for derive feature support
   /// Requires `config` 0.13+ for TOML support
   ```

## Final Dependency Recommendations

### Minimal Viable Dependencies

```toml
[workspace.dependencies]
# Core
serde = { version = "1.0", features = ["derive"] }
thiserror = "1.0"
log = "0.4"

# Configuration
config = "0.13"
toml = "0.7"

# CLI
clap = { version = "4.0", features = ["derive"] }

# Async
tokio = { version = "1.0", features = ["process", "fs"] }

# Git
git2 = "0.17"

# Unix utilities
nix = "0.26"
users = "0.11"

# Command execution
command-group = "1.0"

# Logging
env_logger = "0.10"

# Testing
mockall = "0.11"
assert_cmd = "2.0"
predicates = "2.1"
```

### Optional Dependencies (Add as Needed)

```toml
# Filesystem
fs_extra = "1.2"
tempfile = "3.3"

# Advanced command execution
duct = "0.13"

# Alternative config formats
serde_json = "1.0"
serde_yaml = "0.9"

# Benchmarking
criterion = "0.4"
```

## Dependency Maintenance Plan

### 1. Regular Updates

- **Monthly**: Check for dependency updates
- **Quarterly**: Review dependency usage
- **Annually**: Major dependency review

### 2. Security Monitoring

- Use `cargo audit` regularly
- Monitor security advisories
- Update vulnerable dependencies immediately

### 3. Dependency Review Process

1. **Evaluate New Dependencies**:
   - Maintenance status
   - Community adoption
   - Documentation quality
   - License compatibility

2. **Assess Impact**:
   - Compile time impact
   - Runtime performance
   - Binary size increase
   - Maintenance burden

3. **Document Decisions**:
   - Why the dependency was added
   - What alternatives were considered
   - Migration path if needed

## Conclusion

### Recommended Dependency Strategy

1. **Start Minimal**: Only essential dependencies initially
2. **Add as Needed**: Include optional dependencies when required
3. **Document Everything**: Clear documentation of why each dependency exists
4. **Regular Maintenance**: Keep dependencies updated and secure
5. **Performance Focus**: Optimize for both compile-time and runtime performance

### Final Dependency Count

- **Essential**: 10-12 dependencies
- **Optional**: 5-7 additional dependencies
- **Dev-only**: 4-6 testing dependencies

This approach balances functionality with maintainability, ensuring a robust foundation while keeping the dependency footprint manageable.