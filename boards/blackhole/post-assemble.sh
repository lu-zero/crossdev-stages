set -e

# Stage Image / dtb / fw_jump.bin at /build/ for genimage to pack;
# Blackhole boots them straight into DRAM via PCIe BAR (no /boot).
cp /build/linux/arch/${KERNEL_ARCH}/boot/Image /build/
cp /build/linux/arch/${KERNEL_ARCH}/boot/dts/tenstorrent/*.dtb /build/ 2>/dev/null || true
cp /build/opensbi/build/platform/${OPENSBI_PLATFORM}/firmware/fw_jump.bin /build/

# Other boards mount /dev via initramfs; we boot the kernel directly
# so the on-disk rootfs needs an empty /dev for devtmpfs auto-mount.
mkdir -p /build/gen/root/dev

# hvc0 (virtio_console) has no carrier; agetty needs -L to spawn login.
sed -i 's|/sbin/agetty \([0-9]\+\) hvc0|/sbin/agetty -L \1 hvc0|' \
    /build/gen/root/etc/inittab
