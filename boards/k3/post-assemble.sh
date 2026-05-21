#!/bin/bash
set -e

# Board firmware overlay (WiFi/BT blobs from buildroot-ext)
mkdir -p /build/gen/root/lib/firmware
cp -a /build/firmware/${BOARD_FIRMWARE_OVERLAY}/. /build/gen/root/lib/firmware/

# Host firmware (wifi, etc.)
for fw_path in ${HOST_FIRMWARE_PATHS[@]+"${HOST_FIRMWARE_PATHS[@]}"}; do
    cp -a "${fw_path}" /build/gen/root/lib/firmware/ 2>/dev/null || true
done

# U-Boot environment for K3 bootfs (runtime env override read by U-Boot from filesystem)
printf 'console=%s\ninit=/init\nbootdelay=0\nloglevel=%s\nknl_name=%s\nramdisk_name=%s\nset_root_arg=setenv bootargs root=%s\n' \
    "${BOOT_CONSOLE}" "${BOOT_LOGLEVEL}" "${BOOT_KERNEL_NAME}" "${BOOT_RAMDISK_NAME}" "${BOOT_ROOT_DEV}" \
    > /build/gen/boot/env_k3.txt

# Stage pre-built ESOS firmware (auxiliary core — not built from source)
cp /scripts/boards/k3/firmware/esos.itb /build/esos.itb

# Stage factory/ partition images from u-boot build outputs
mkdir -p /build/factory
cp /build/u-boot/FSBL.bin            /build/factory/FSBL.bin
cp /build/u-boot/bootinfo_block.bin  /build/factory/bootinfo_block.bin

# Create env.bin for the env partition from the u-boot build's default environment
cp /build/u-boot/u-boot-env-default.bin /build/u-boot/env.bin

# Build perf from kernel source (version-matched to K3 kernel) and install to rootfs
make -C /build/linux/tools/perf \
    ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" \
    V=1 WERROR=0 NO_LIBPYTHON=1 NO_LIBPERL=1 NO_LIBTRACEEVENT=1 \
    DESTDIR=/build/gen/root/usr install
