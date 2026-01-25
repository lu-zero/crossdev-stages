# Usage Examples for Refactored crossdev-stages

## Basic Usage (Same as Before)

The refactored scripts maintain full backward compatibility. Existing users can continue using them exactly as before:

```bash
# Setup cross-compilation environment (requires root)
sudo ./cross-stage.sh prepare

# Create a new stage1
sudo ./cross-stage.sh make /path/to/stage

# Update an existing stage3
sudo ./cross-stage.sh update /path/to/stage

# Install additional packages
sudo ./cross-stage.sh install_more /path/to/stage

# Build bootable image
./make-image.sh /path/to/build /path/to/stage
```

## New Features

### 1. Platform Configuration

#### Using default platform (riscv64-k1)
```bash
# This is the same as the basic usage above
# The default platform is automatically loaded
sudo ./cross-stage.sh make /path/to/stage
```

#### Using a specific platform
```bash
# Explicitly specify the platform
sudo ./cross-stage.sh --platform riscv64-k1 make /path/to/stage

# Build image for specific platform
./make-image.sh --platform riscv64-k1 /build /stage
```

#### Using a custom configuration file
```bash
# Use a custom configuration file
sudo ./cross-stage.sh --config config/platforms/my-custom-platform.conf make /path/to/stage
```

### 2. Help and Information

#### Getting help
```bash
# Show help for cross-stage.sh
./cross-stage.sh --help

# Show help for make-image.sh
./make-image.sh --help
```

#### Example help output
```
Usage: ./cross-stage.sh [options] <command> [stage-directory]

Options:
  --config,-c <file>  Use alternative configuration file
  --platform,-p <name> Use specific platform configuration
  --help,-h           Show this help message

Commands:
  prepare             Setup crossdev environment
  make               Create a new stage1
  update             Update a pre-existing stage3
  update_ldconfig    Update ldconfig cache
  install_clang      Install clang in the stage
  install_boot       Install the bootloader requirements
  install_more       Install additional starting packages
  install_perl       Install perl

Examples:
  ./cross-stage.sh --help                          Show this help
  ./cross-stage.sh prepare                         Setup crossdev environment
  ./cross-stage.sh make /path/to/stage             Create a new stage1
  ./cross-stage.sh --platform riscv64-k1 make /path/to/stage
```

### 3. Configuration Management

#### Viewing current configuration
```bash
# Load and display configuration
source lib/common.sh
load_config config/platforms/riscv64-k1.conf
echo "TARGET_ARCH: $TARGET_ARCH"
echo "TARGET_CHOST: $TARGET_CHOST"
echo "CFLAGS: $CFLAGS"
```

#### Creating a new platform configuration
```bash
# Copy existing configuration as template
cp config/platforms/riscv64-k1.conf config/platforms/arm64-rpi.conf

# Edit the new configuration
# Change TARGET_ARCH, TARGET_CHOST, CFLAGS, repository URLs, etc.
sed -i '' 's/riscv64/arm64/g' config/platforms/arm64-rpi.conf
sed -i '' 's/riscv64-unknown-linux-gnu/aarch64-unknown-linux-gnu/g' config/platforms/arm64-rpi.conf
sed -i '' 's/rv64gcv_zvl256b/cortex-a72/g' config/platforms/arm64-rpi.conf

# Update repository URLs for ARM64
sed -i '' 's|k1-opensbi|rpi4-opensbi|g' config/platforms/arm64-rpi.conf
sed -i '' 's|k1-bl-v2.2.7-release|rpi4-bl-v1.0.0|g' config/platforms/arm64-rpi.conf

# Use the new platform
sudo ./cross-stage.sh --platform arm64-rpi make /path/to/arm64-stage
```

### 4. Package Management

#### Viewing package lists
```bash
# View base packages
cat config/packages/stage1-packages.txt

# View additional packages
cat config/packages/additional-packages.txt
```

#### Customizing package lists
```bash
# Add a package to additional packages
echo "app-editors/nano" >> config/packages/additional-packages.txt

# Remove a package from additional packages
sed -i '' '/app-editors\/vim/d' config/packages/additional-packages.txt
```

### 5. Advanced Usage

#### Building for multiple platforms
```bash
# Build RISC-V stage
sudo ./cross-stage.sh --platform riscv64-k1 make /path/to/riscv-stage

# Build ARM64 stage (after creating arm64 config)
sudo ./cross-stage.sh --platform arm64-rpi make /path/to/arm64-stage

# Build images for both platforms
./make-image.sh --platform riscv64-k1 /build-riscv /path/to/riscv-stage
./make-image.sh --platform arm64-rpi /build-arm64 /path/to/arm64-stage
```

#### Using custom genimage configuration
```bash
# Create a custom genimage configuration
cp config/genimage-k1.cfg config/genimage-custom.cfg

# Edit the custom configuration
# Change partition sizes, layouts, etc.
sed -i '' 's/size = 5G/size = 10G/g' config/genimage-custom.cfg

# Update platform configuration to use custom genimage
sed -i '' 's|GENIMAGE_CONFIG="config/genimage-k1.cfg"|GENIMAGE_CONFIG="config/genimage-custom.cfg"|g' config/platforms/riscv64-k1.conf

# Build with custom image configuration
./make-image.sh /build /stage
```

## Configuration File Structure

### Platform Configuration (`config/platforms/riscv64-k1.conf`)
```bash
# Target architecture
TARGET_ARCH="riscv64"
TARGET_CHOST="riscv64-unknown-linux-gnu"
TARGET_FLAVOR="rv64_lp64d-openrc"
TARGET_KEYWORD="riscv"

# Cross-compilation settings
CROSS_COMPILE="${TARGET_CHOST}-"
CFLAGS="-O3 -march=rv64gcv_zvl256b -pipe"
GCC_VERSION="16.0.0_p20251005"

# Gentoo profile
GENTOO_PROFILE="default/linux/riscv/23.0/rv64/lp64d"

# Bootloader and kernel sources
OPENSBI_REPO="https://github.com/cyyself/opensbi"
OPENSBI_TAG="k1-opensbi"
U_BOOT_REPO="https://gitee.com/bianbu-linux/uboot-2022.10.git"
FIRMWARE_REPO="https://gitee.com/bianbu-linux/buildroot-ext.git"
KERNEL_REPO="https://gitee.com/bianbu-linux/linux-6.6.git"
BOOTLOADER_TAG="k1-bl-v2.2.7-release"

# Package lists
STAGE1_PACKAGES_FILE="config/packages/stage1-packages.txt"
ADDITIONAL_PACKAGES_FILE="config/packages/additional-packages.txt"

# Image configuration
IMAGE_SIZE_ROOT="5G"
IMAGE_SIZE_BOOT="500M"
GENIMAGE_CONFIG="config/genimage-k1.cfg"
```

### Package Lists

#### Base packages (`config/packages/stage1-packages.txt`)
```
sys-apps/baselayout
sys-apps/portage
sys-apps/coreutils
sys-apps/findutils
# ... more base packages
```

#### Additional packages (`config/packages/additional-packages.txt`)
```
sys-block/parted
app-editors/vim
app-admin/metalog
# ... more additional packages
```

## Workflow Comparison: Before vs After

### Before Refactoring
```bash
# Hardcoded RISC-V configuration
# To change platform, you had to edit the scripts
sed -i 's/riscv64/arm64/g' cross-stage.sh
sed -i 's/rv64gcv_zvl256b/cortex-a72/g' cross-stage.sh
# ... many more manual changes

# No easy way to switch between platforms
# Configuration mixed with functionality
```

### After Refactoring
```bash
# Create new platform configuration
cp config/platforms/riscv64-k1.conf config/platforms/arm64-rpi.conf
sed -i '' 's/riscv64/arm64/g' config/platforms/arm64-rpi.conf
sed -i '' 's/rv64gcv_zvl256b/cortex-a72/g' config/platforms/arm64-rpi.conf

# Use the new platform
./cross-stage.sh --platform arm64-rpi make /stage
./make-image.sh --platform arm64-rpi /build /stage

# Switch back to RISC-V
./cross-stage.sh --platform riscv64-k1 make /stage
```

## Benefits in Practice

### 1. Easy Platform Switching
```bash
# Before: Edit scripts, risk breaking things
# After: Simple command line option
./cross-stage.sh --platform arm64-rpi make /stage
```

### 2. Configuration Management
```bash
# Before: Configuration scattered in scripts
# After: Centralized configuration files
vim config/platforms/riscv64-k1.conf
```

### 3. Package Customization
```bash
# Before: Edit script variables
# After: Edit package list files
vim config/packages/additional-packages.txt
```

### 4. Image Customization
```bash
# Before: Edit genimage.cfg directly
# After: Platform-specific genimage configurations
cp config/genimage-k1.cfg config/genimage-large.cfg
sed -i '' 's/5G/20G/' config/genimage-large.cfg
```

## Troubleshooting

### Configuration not loading
```bash
# Check if configuration file exists
ls -la config/platforms/riscv64-k1.conf

# Check file permissions
chmod 644 config/platforms/riscv64-k1.conf

# Verify configuration syntax
bash -n config/platforms/riscv64-k1.conf
```

### Platform not found
```bash
# List available platforms
ls config/platforms/

# Check platform name spelling
./cross-stage.sh --platform riscv64-k1 --help
```

### Missing packages
```bash
# Check package list files
cat config/packages/stage1-packages.txt
cat config/packages/additional-packages.txt

# Add missing packages
echo "missing-package" >> config/packages/additional-packages.txt
```

## Best Practices

### 1. Version Control
```bash
# Keep configuration files in version control
git add config/
git commit -m "Add RISC-V K1 platform configuration"
```

### 2. Platform Templates
```bash
# Create template for new platforms
cp -r config/platforms/riscv64-k1.conf config/platforms/template.conf
sed -i '' 's/riscv64/PLATFORM_NAME/g' config/platforms/template.conf
```

### 3. Configuration Validation
```bash
# Validate configuration before use
source lib/common.sh
if load_config config/platforms/my-platform.conf; then
    echo "Configuration valid"
    # Check required variables
    [[ -n "$TARGET_ARCH" ]] && [[ -n "$TARGET_CHOST" ]] && echo "Required variables set"
fi
```

### 4. Documentation
```bash
# Document platform-specific requirements
# Add comments to configuration files
# Create README for each platform
```

## Summary

The refactored system provides:
- **Simpler usage** for common tasks (backward compatible)
- **More flexibility** for advanced use cases (platform switching)
- **Better organization** of configuration vs functionality
- **Easier maintenance** through centralized configuration
- **Improved documentation** with help messages and examples

Existing users can continue using the scripts exactly as before, while new users and advanced use cases benefit from the improved flexibility and organization.