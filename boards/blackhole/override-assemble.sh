set -e

mkdir -p /build/gen/root /build/gen/boot

# Copy target sysroot
cp -a /target/. /build/gen/root/

# Install kernel modules
make -C /build/linux ARCH=${KERNEL_ARCH} CROSS_COMPILE=${CROSS_COMPILE} \
    INSTALL_MOD_PATH=/build/gen/root modules_install

# Copy kernel Image and DTBs to build root (host tool loads via PCIe BAR)
cp /build/linux/arch/${KERNEL_ARCH}/boot/Image /build/
cp /build/linux/arch/${KERNEL_ARCH}/boot/dts/tenstorrent/*.dtb /build/ 2>/dev/null || true

# Copy opensbi fw_jump
cp /build/opensbi/build/platform/${OPENSBI_PLATFORM}/firmware/fw_jump.bin /build/

# ldconfig
${LDCONFIG} -v -r /build/gen/root
