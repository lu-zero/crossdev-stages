# Refactoring Summary: Separating Configuration from Functionality

## Overview
This refactoring separates the platform-specific configuration from the core functionality, making the scripts more maintainable, flexible, and easier to adapt to different platforms.

## Key Changes

### 1. Configuration System
- **Created platform-specific configuration files**: `config/platforms/riscv64-k1.conf`
- **Created package list files**: 
  - `config/packages/stage1-packages.txt`
  - `config/packages/additional-packages.txt`
- **Created platform-specific genimage config**: `config/genimage-k1.cfg`

### 2. Common Library
- **Created `lib/common.sh`**: Contains shared functions used by all scripts
- **Key functions**:
  - `load_config()`: Load configuration from external files
  - `gentoo_arch()`: Map OS architecture to Gentoo architecture
  - `run_bwrap()`: Run commands in bubblewrap containers
  - `check_root()`: Verify root privileges
  - `read_package_list()`: Read package lists from files
  - `setup_crossdev_env()`: Setup cross-compilation environment
  - `prepare_stage1()`: Prepare stage1 environment
  - `install_stage1()`: Install stage1 packages
  - `update_ldconfig()`: Update ldconfig cache
  - `checkout_repo()`: Checkout git repositories
  - `parse_args()`: Parse command line arguments (deprecated, replaced with inline parsing)

### 3. Refactored Scripts

#### cross-stage.sh
- **Before**: Hardcoded RISC-V specific configuration
- **After**: Uses external configuration files
- **New features**:
  - `--config,-c <file>`: Use alternative configuration file
  - `--platform,-p <name>`: Use specific platform configuration
  - `--help,-h`: Show help message
  - Better error handling and usage information

#### make-image.sh
- **Before**: Hardcoded repository URLs, tags, and build settings
- **After**: Uses external configuration files
- **New features**:
  - Same command line options as cross-stage.sh
  - Platform-specific genimage configuration
  - Better help and usage information

### 4. Configuration Structure

```
config/
├── platforms/
│   └── riscv64-k1.conf          # Platform-specific settings
├── packages/
│   ├── stage1-packages.txt      # Base package list
│   └── additional-packages.txt  # Additional packages
└── genimage-k1.cfg             # Platform-specific image config
```

### 5. Configuration Variables
Key variables now defined in configuration files:
- `TARGET_ARCH`, `TARGET_CHOST`, `TARGET_FLAVOR`, `TARGET_KEYWORD`
- `CROSS_COMPILE`, `CFLAGS`, `GCC_VERSION`
- `GENTOO_PROFILE`
- Repository URLs and tags (`OPENSBI_REPO`, `U_BOOT_REPO`, etc.)
- Package list file paths
- Image size configurations

## Benefits

1. **Platform Independence**: Easy to add support for new platforms by creating new configuration files
2. **Maintainability**: Configuration changes don't require script modifications
3. **Flexibility**: Users can override configuration via command line options
4. **Reusability**: Common functions are shared across scripts
5. **Testability**: Configuration can be tested independently
6. **Documentation**: Better help messages and usage examples

## Usage Examples

### Using default configuration
```bash
# Build cross-compilation environment
sudo ./cross-stage.sh prepare

# Create stage1
sudo ./cross-stage.sh make /path/to/stage

# Build bootable image
./make-image.sh /path/to/build /path/to/stage
```

### Using custom platform configuration
```bash
# Use specific platform
sudo ./cross-stage.sh --platform riscv64-k1 make /path/to/stage

# Or use custom config file
sudo ./cross-stage.sh --config config/platforms/my-platform.conf make /path/to/stage
```

### Getting help
```bash
./cross-stage.sh --help
./make-image.sh --help
```

## Migration Guide

### For existing users
1. The default behavior remains the same (uses riscv64-k1 configuration)
2. Existing command line arguments are preserved
3. No changes needed for existing workflows

### For adding new platforms
1. Copy `config/platforms/riscv64-k1.conf` to `config/platforms/<platform>.conf`
2. Modify the configuration variables as needed
3. Create platform-specific package lists if needed
4. Create platform-specific genimage configuration if needed
5. Use `--platform <platform>` to select the new platform

## Testing

The refactoring includes test scripts:
- `test-config.sh`: Tests configuration loading and basic functions
- `test-platform-config.sh`: Tests platform configuration switching

## Future Enhancements

1. **Additional platforms**: Easy to add ARM, x86, etc. by creating new config files
2. **Configuration validation**: Add schema validation for configuration files
3. **Environment variable support**: Allow configuration via environment variables
4. **JSON/YAML support**: Alternative configuration formats
5. **Automatic platform detection**: Detect target platform automatically

## Files Modified

- `cross-stage.sh`: Refactored to use external configuration
- `make-image.sh`: Refactored to use external configuration
- `genimage.cfg`: Converted to template, platform-specific version created
- `lib/common.sh`: New file with shared functions

## Files Added

- `config/platforms/riscv64-k1.conf`: RISC-V K1 platform configuration
- `config/packages/stage1-packages.txt`: Base package list
- `config/packages/additional-packages.txt`: Additional packages list
- `config/genimage-k1.cfg`: Platform-specific genimage configuration
- `test-config.sh`: Configuration testing script
- `test-platform-config.sh`: Platform configuration testing script

## Backward Compatibility

The refactoring maintains full backward compatibility:
- Default behavior unchanged
- Existing command line arguments work as before
- No breaking changes to the API or functionality