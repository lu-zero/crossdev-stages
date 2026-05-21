#!/bin/bash
set -e

# Board firmware overlay (WiFi/BT blobs from buildroot-ext)
mkdir -p /build/gen/root/lib/firmware
cp -a /build/firmware/${BOARD_FIRMWARE_OVERLAY}/. /build/gen/root/lib/firmware/

# Host firmware (wifi, etc.)
for fw_path in ${HOST_FIRMWARE_PATHS[@]+"${HOST_FIRMWARE_PATHS[@]}"}; do
    cp -a "${fw_path}" /build/gen/root/lib/firmware/ 2>/dev/null || true
done

# Move DTBs into spacemit/${kver}/ to match the path layout the K3 u-boot
# env's `loaddtb` expects (`${dtb_dir}/${dtb_name}`).
kver=$(ls /build/gen/root/lib/modules/ 2>/dev/null | head -1)
mkdir -p "/build/gen/boot/spacemit/${kver}"
mv /build/gen/boot/*.dtb "/build/gen/boot/spacemit/${kver}/" 2>/dev/null || true

# U-Boot environment override read by u-boot from bootfs at boot time.
# Matches vendor Bianbu/Debian env_k3.txt convention.
#
# Intentionally OMITS `set_root_arg` — the compiled-in default in env.bin
# resolves the rootfs partition's GPT UUID at runtime via
#   `part uuid ${boot_devname} ${boot_devnum}:${rootfs_part} rootfs_guid`
# and sets `root=PARTUUID=${rootfs_guid}`.  PARTUUID works without
# initramfs/udev (LABEL=/UUID= do not).
#
# Also omits set_console / set_loglevel — vendor env has functions that
# append them only when the var is non-empty; we let those defaults apply.
printf 'knl_name=%s\nramdisk_name=%s\ndtb_dir=spacemit/%s\ndtb_name=%s\nramdisk_addr=0x130000000\nloglevel=%s\ncommonargs=setenv bootargs earlycon=sbi earlyprintk console=%s clk_ignore_unused random.trust_bootloader=1\n' \
    "${BOOT_KERNEL_NAME}" "${BOOT_RAMDISK_NAME}" "${kver}" "${BOOT_DTB_NAME}" \
    "${BOOT_LOGLEVEL}" "${BOOT_CONSOLE}" \
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
