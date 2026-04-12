set -e

# Board firmware overlay
mkdir -p /build/gen/root/lib/firmware
cp -a /build/firmware/${BOARD_FIRMWARE_OVERLAY}/. /build/gen/root/lib/firmware/

# Host firmware (wifi, etc.)
for fw_path in ${HOST_FIRMWARE_PATHS[@]+"${HOST_FIRMWARE_PATHS[@]}"}; do
    cp -a "${fw_path}" /build/gen/root/lib/firmware/ 2>/dev/null || true
done

# Dracut firmware hint
mkdir -p /build/gen/root/etc/dracut.conf.d
echo 'install_items+=" /lib/firmware/esos.elf "' > /build/gen/root/etc/dracut.conf.d/firmware.conf

# U-Boot boot script + uInitrd
mkimage -A riscv -T script -C none -d /scripts/boards/ky-x1/boot.cmd /build/gen/boot/boot.scr
mkimage -A riscv -O linux -T ramdisk -C gzip -d /build/gen/boot/initramfs.img /build/gen/boot/uInitrd
