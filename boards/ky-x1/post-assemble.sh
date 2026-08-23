set -e

# Dracut firmware hint
mkdir -p /build/gen/root/etc/dracut.conf.d
echo 'install_items+=" /lib/firmware/esos.elf "' > /build/gen/root/etc/dracut.conf.d/firmware.conf

# U-Boot boot script + uInitrd
mkimage -A riscv -T script -C none -d /scripts/boards/ky-x1/boot.cmd /build/gen/boot/boot.scr
mkimage -A riscv -O linux -T ramdisk -C gzip -d /build/gen/boot/initramfs.img /build/gen/boot/uInitrd
