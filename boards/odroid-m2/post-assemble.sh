set -e

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
    FDT /rk3588s-odroid-m2.dtb
    APPEND root=${BOOT_ROOT_DEV} rw rootwait rootfstype=ext4 console=${BOOT_CONSOLE} earlycon
EXTEOF
