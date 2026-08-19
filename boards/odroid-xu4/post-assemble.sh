set -e

# Mainline U-Boot's odroid-xu3 target keeps CONFIG_DISTRO_DEFAULTS, whose
# BOOT_TARGET_DEVICES walks mmc2 (the SD slot), mmc1 and mmc0 looking for
# extlinux/extlinux.conf.  Hardkernel's boot.ini is a fork-only feature and is
# not needed.  DISTRO_DEFAULTS also pulls in CMD_BOOTZ on 32-bit ARM, so a raw
# zImage boots without being wrapped as a uImage.
mkdir -p /build/gen/boot/extlinux
kver=$(ls /build/gen/root/lib/modules/ | head -1)
[ -z "$kver" ] && { echo 'Error: no kernel modules found'; exit 1; }
cat > /build/gen/boot/extlinux/extlinux.conf << EXTEOF
DEFAULT gentoo
TIMEOUT 30
LABEL gentoo
    MENU LABEL Gentoo Linux
    LINUX /${BOOT_KERNEL_NAME}
    FDT /exynos5422-odroidxu4.dtb
    APPEND root=${BOOT_ROOT_DEV} rw rootwait rootfstype=ext4 console=${BOOT_CONSOLE} earlycon
EXTEOF
