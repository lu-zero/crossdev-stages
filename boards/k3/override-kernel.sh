#!/bin/bash
set -e

# K3 kernel build with GCC plugins disabled — Linux 6.18's plugin sources
# use the pre-gcc-16 plugin C++ API (CONST_CAST_TREE etc.); they fail to
# compile when the host gcc is 16+.  Disable until upstream catches up.
cd /build/linux
make ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" "${KERNEL_DEFCONFIG}"
scripts/config --disable GCC_PLUGINS
make ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" olddefconfig
make ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" -j"$(nproc)"
