set -e

# Firmware, paths preserved: panthor asks for arm/mali/arch<N>.<M>/mali_csffw.bin,
# and the wireless drivers are just as picky about their subdirectory.
for dir in "${FIRMWARE_DIRS[@]}"; do
    [ -d "/build/firmware/${dir}" ] || { echo "Error: no firmware dir ${dir}"; exit 1; }
    mkdir -p "/build/gen/root/lib/firmware/${dir}"
    cp -a "/build/firmware/${dir}/." "/build/gen/root/lib/firmware/${dir}/"
done

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
    FDT /rk3588-orangepi-5-ultra.dtb
    APPEND root=${BOOT_ROOT_DEV} rw rootwait rootfstype=ext4 console=${BOOT_CONSOLE} earlycon cma=256M
EXTEOF
