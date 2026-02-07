# Sandbox State Tracking System

## Overview

The sandbox state tracking system provides a way to monitor and manage the state of Docker sandboxes used by crossdev-stages. It tracks:

- Which sandboxes exist
- Their current state (new, prepared, stage_loaded, updating, error)
- Which stages are loaded in each sandbox
- Timestamps for creation and last updates

## Data Structures

### SandboxState

```rust
pub struct SandboxState {
    pub name: String,              // Sandbox name
    pub state: SandboxStatus,      // Current state
    pub loaded_stage: Option<String>, // Optional loaded stage
    pub last_updated: String,      // Timestamp (YYYYMMDDTHH format)
    pub created_at: String,        // Creation timestamp
}
```

### SandboxStatus

```rust
pub enum SandboxStatus {
    New,           // Sandbox created but not prepared
    Prepared,      // Sandbox prepared for use
    StageLoaded,   // Stage loaded into sandbox
    Updating,      // Sandbox is being updated
    Error,         // Sandbox in error state
}
```

### SandboxRegistry

```rust
pub struct SandboxRegistry {
    sandboxes: Vec<SandboxState>,  // List of all sandboxes
}
```

## Storage

The sandbox registry is stored as JSON at:
- **Linux**: `~/.local/state/crossdev-stages/sandboxes.json`
- **macOS**: `~/Library/Application Support/crossdev-stages/sandboxes.json`
- **Windows**: `%APPDATA%\crossdev-stages\sandboxes.json`

## API

### Registry Management

```rust
// Load registry from file (creates empty if doesn't exist)
let registry = SandboxRegistry::load_from_file(&path)?;

// Save registry to file
registry.save_to_file(&path)?;

// Get default registry path
let path = SandboxRegistry::get_default_registry_path();
```

### Sandbox Management

```rust
// Create a new sandbox state
let sandbox = SandboxRegistry::create_sandbox_state("my-sandbox", SandboxStatus::New);

// Add or update a sandbox
registry.upsert_sandbox(sandbox)?;

// Get a sandbox by name
if let Some(sandbox) = registry.get_sandbox("my-sandbox") {
    println!("Found sandbox: {:?}", sandbox.state);
}

// List all sandboxes
for sandbox in registry.list_sandboxes() {
    println!("Sandbox: {} - {:?}", sandbox.name, sandbox.state);
}

// Remove a sandbox
let removed = registry.remove_sandbox("my-sandbox")?;
```

## Integration Examples

### Tracking Sandbox Creation

```rust
// When creating a new sandbox
let mut registry = SandboxRegistry::load_from_file(&registry_path)?;
let new_sandbox = SandboxRegistry::create_sandbox_state("default", SandboxStatus::New);
registry.upsert_sandbox(new_sandbox)?;
registry.save_to_file(&registry_path)?;
```

### Tracking Stage Loading

```rust
// When loading a stage into a sandbox
let mut registry = SandboxRegistry::load_from_file(&registry_path)?;
if let Some(mut sandbox) = registry.get_sandbox("default").cloned() {
    sandbox.state = SandboxStatus::StageLoaded;
    sandbox.loaded_stage = Some("stage3-riscv64-k1-20240101".to_string());
    sandbox.last_updated = Timestamp::now().strftime("%Y%m%dT%H").to_string();
    registry.upsert_sandbox(sandbox)?;
    registry.save_to_file(&registry_path)?;
}
```

### Tracking Updates

```rust
// When updating a sandbox
let mut registry = SandboxRegistry::load_from_file(&registry_path)?;
if let Some(mut sandbox) = registry.get_sandbox("default").cloned() {
    sandbox.state = SandboxStatus::Updating;
    sandbox.last_updated = Timestamp::now().strftime("%Y%m%dT%H").to_string();
    registry.upsert_sandbox(sandbox)?;
    registry.save_to_file(&registry_path)?;

    // After successful update
    sandbox.state = SandboxStatus::StageLoaded;
    sandbox.last_updated = Timestamp::now().strftime("%Y%m%dT%H").to_string();
    registry.upsert_sandbox(sandbox)?;
    registry.save_to_file(&registry_path)?;
}
```

## Benefits

1. **State Awareness**: Know the current state of all sandboxes
2. **Stage Tracking**: Track which stages are loaded where
3. **Debugging**: Helpful for debugging and troubleshooting
4. **Recovery**: Assist in recovery from errors
5. **Monitoring**: Enable monitoring of sandbox lifecycle

## Future Enhancements

- Add CLI commands to query sandbox states
- Integrate with update process to track updates
- Add cleanup for removed sandboxes
- Implement state validation and repair
- Add timestamp-based cleanup of old entries

## Current Status

The sandbox state tracking system is implemented but not yet integrated into the main CLI workflow. It provides the foundation for better sandbox management and can be integrated as needed.