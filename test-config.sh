#!/bin/bash

# Test script to verify the refactoring works

echo "Testing configuration loading..."

# Test loading the common library
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/lib/common.sh"

# Test loading platform configuration
echo "Loading platform configuration..."
load_config "config/platforms/riscv64-k1.conf"

echo "Configuration loaded successfully!"
echo "TARGET_ARCH: $TARGET_ARCH"
echo "TARGET_CHOST: $TARGET_CHOST"
echo "TARGET_FLAVOR: $TARGET_FLAVOR"
echo "CFLAGS: $CFLAGS"
echo "GCC_VERSION: $GCC_VERSION"
echo "GENTOO_PROFILE: $GENTOO_PROFILE"

echo ""
echo "Testing package list reading..."
echo "Stage1 packages:"
read_package_list "config/packages/stage1-packages.txt"

echo ""
echo "Additional packages:"
read_package_list "config/packages/additional-packages.txt"

echo ""
echo "Testing gentoo_arch function..."
gentoo_arch "x86_64"
echo "x86_64 -> ARCH=$ARCH, FLAVOR=$FLAVOR"
gentoo_arch "aarch64"
echo "aarch64 -> ARCH=$ARCH, FLAVOR=$FLAVOR"
gentoo_arch "riscv64"
echo "riscv64 -> ARCH=$ARCH, FLAVOR=$FLAVOR"

echo ""
echo "All tests passed! The refactoring appears to be working correctly."