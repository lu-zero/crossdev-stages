set -e

# U-Boot boot script + uInitrd (initramfs.img already built by default_assemble).
mkimage -A riscv -T script -C none -d /scripts/boards/ky-x1/boot.cmd /build/gen/boot/boot.scr
mkimage -A riscv -O linux -T ramdisk -C gzip -d /build/gen/boot/initramfs.img /build/gen/boot/uInitrd
