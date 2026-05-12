set -e

# Ensure virtual-fs mountpoints exist in the rootfs.
# cross-emerged baselayout doesn't reliably create these, and without /dev
# the kernel can't auto-mount devtmpfs, leaving init with no /dev/console
# (boot silently hangs after "Run /sbin/init as init process").
mkdir -p /build/gen/root/dev \
         /build/gen/root/proc \
         /build/gen/root/sys \
         /build/gen/root/run \
         /build/gen/root/tmp \
         /build/gen/root/mnt \
         /build/gen/root/media
chmod 1777 /build/gen/root/tmp

# Extlinux boot config
mkdir -p /build/gen/boot/extlinux
kver=$(ls /build/gen/root/lib/modules/ | head -1)
[ -z "$kver" ] && { echo 'Error: no kernel modules found'; exit 1; }
cat > /build/gen/boot/extlinux/extlinux.conf << EXTEOF
DEFAULT gentoo
TIMEOUT 30
LABEL gentoo
    MENU LABEL Gentoo Linux
    LINUX /${BOOT_KERNEL_NAME}
    FDT /rk3568-odroid-m1.dtb
    APPEND root=${BOOT_ROOT_DEV} rw rootwait rootfstype=ext4 console=${BOOT_CONSOLE} earlycon
EXTEOF
