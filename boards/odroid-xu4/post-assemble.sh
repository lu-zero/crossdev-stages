set -e

# Firmware, paths preserved: r8152 asks for rtl_nic/rtl8153a-3.fw by name.
for dir in "${FIRMWARE_DIRS[@]}"; do
    [ -d "/build/firmware/${dir}" ] || { echo "Error: no firmware dir ${dir}"; exit 1; }
    mkdir -p "/build/gen/root/lib/firmware/${dir}"
    cp -a "/build/firmware/${dir}/." "/build/gen/root/lib/firmware/${dir}/"
done

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
    APPEND root=${BOOT_ROOT_DEV} rw rootwait rootfstype=ext4 console=tty1 console=${BOOT_CONSOLE} earlycon
EXTEOF

# The stage3 fstab is comments only, so /boot never mounts and kernel updates
# would land in the rootfs copy instead of the partition u-boot reads.
# Same PARTUUID scheme as root: index-agnostic, so the card boots the same
# whether or not an eMMC module is fitted.
cat >> /build/gen/root/etc/fstab <<FSTAB
PARTUUID=5422b001-02  /      ext4  defaults,noatime  0 1
PARTUUID=5422b001-01  /boot  ext4  defaults,noatime  0 2
FSTAB
