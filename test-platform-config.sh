#!/bin/bash

# Test script to verify platform configuration loading

echo "Testing platform configuration loading..."

# Test loading the common library
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/lib/common.sh"

echo "Testing default platform (riscv64-k1)..."
load_config "config/platforms/riscv64-k1.conf"
echo "TARGET_ARCH: $TARGET_ARCH"
echo "TARGET_CHOST: $TARGET_CHOST"

echo ""
echo "Testing with command line arguments..."

# Test the cross-stage script with different platform
echo "Testing cross-stage.sh with default platform:"
./cross-stage.sh --help | grep "riscv64-k1" || echo "Default platform not shown in help"

echo ""
echo "Testing cross-stage.sh with custom platform:"
./cross-stage.sh --platform riscv64-k1 --help | grep "riscv64-k1" || echo "Custom platform not shown in help"

echo ""
echo "All platform configuration tests completed!"