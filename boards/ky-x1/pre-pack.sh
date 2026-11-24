set -e

# U-Boot ramdisk wrapper around the initramfs assemble built
mkimage -A riscv -O linux -T ramdisk -C gzip -d /build/gen/boot/initramfs.img /build/gen/boot/uInitrd
