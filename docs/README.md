# crossdev-stages Documentation

This directory contains documentation for the refactored crossdev-stages project.

## Overview

The crossdev-stages project has been refactored to separate configuration from functionality, making it more maintainable, flexible, and easier to adapt to different platforms.

## Documentation Files

### 📋 [REFACTORING_SUMMARY.md](REFACTORING_SUMMARY.md)
Comprehensive summary of all refactoring changes, including:
- Configuration system architecture
- Common library functions
- Refactored scripts details
- Benefits and usage examples
- Migration guide for existing users

### 📖 [USAGE_EXAMPLES.md](USAGE_EXAMPLES.md)
Practical usage examples showing:
- Basic usage (backward compatible)
- New features and command line options
- Platform configuration management
- Package customization
- Advanced usage patterns
- Troubleshooting guide

### 🎯 [DEMO_KEY_IMPROVEMENTS.md](DEMO_KEY_IMPROVEMENTS.md)
Side-by-side comparison of improvements:
- Before/after code examples
- Key benefits demonstration
- Practical examples
- Impact analysis
- Summary of improvements

## Quick Start

### Basic Usage (Same as Before)
```bash
# Setup cross-compilation environment
sudo ./cross-stage.sh prepare

# Create stage1
sudo ./cross-stage.sh make /path/to/stage

# Build bootable image
./make-image.sh /path/to/build /path/to/stage
```

### New Features
```bash
# Use specific platform
sudo ./cross-stage.sh --platform riscv64-k1 make /stage

# Get comprehensive help
./cross-stage.sh --help

# Use custom configuration
sudo ./cross-stage.sh --config my-config.conf make /stage
```

## Configuration Structure

```
config/
├── platforms/          # Platform-specific configurations
│   └── riscv64-k1.conf  # RISC-V K1 platform settings
├── packages/           # Package lists
│   ├── stage1-packages.txt      # Base packages
│   └── additional-packages.txt  # Additional packages
└── genimage-k1.cfg     # Platform-specific image config

lib/
└── common.sh           # Shared functions library

docs/                   # Documentation (this directory)
├── REFACTORING_SUMMARY.md
├── USAGE_EXAMPLES.md
└── DEMO_KEY_IMPROVEMENTS.md
```

## Key Improvements

1. **Configuration Separation**: Platform-specific settings in external files
2. **Platform Switching**: Easy `--platform` command line option
3. **Package Management**: External package list files
4. **Help System**: Comprehensive help with examples
5. **Code Organization**: Shared library reduces duplication
6. **Error Handling**: Improved validation and messages
7. **CLI Options**: Rich command line interface
8. **Structure**: Organized directory layout
9. **Backward Compatibility**: 100% compatible with existing usage

## Migration Guide

### For Existing Users
- ✅ No changes required - existing commands work exactly the same
- ✅ Same workflow - no learning curve
- ✅ Backward compatible - no breaking changes

### For New Platforms
1. Copy existing platform configuration
2. Modify settings for new platform
3. Use `--platform` flag to select it

## Support

For issues or questions:
- Check the comprehensive help: `./cross-stage.sh --help`
- Review the usage examples in [USAGE_EXAMPLES.md](USAGE_EXAMPLES.md)
- Consult the troubleshooting guide in usage examples

## License

This documentation is part of the crossdev-stages project and is licensed under the same terms as the main project.