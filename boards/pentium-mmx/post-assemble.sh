set -e

# Create extlinux boot configuration.
# The partition-level bootloader (extlinux/GRUB) must be installed
# separately — this just writes the config file it will read.
mkdir -p /build/gen/boot/extlinux
kver=$(ls /build/gen/root/lib/modules/ | head -1)
[ -z "$kver" ] && { echo 'Error: no kernel modules found'; exit 1; }
cat > /build/gen/boot/extlinux/extlinux.conf << EXTEOF
DEFAULT gentoo
TIMEOUT 30
PROMPT 0
LABEL gentoo
    LINUX /${BOOT_KERNEL_NAME}
    APPEND root=${BOOT_ROOT_DEV} rw rootfstype=ext4 console=${BOOT_CONSOLE}
EXTEOF
