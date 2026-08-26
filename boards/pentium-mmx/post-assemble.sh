set -e

kver=$(ls /build/gen/root/lib/modules/ | head -1)
[ -z "$kver" ] && { echo 'Error: no kernel modules found'; exit 1; }

# Derive chost from CROSS_COMPILE (strip trailing dash).
CHOST="${CROSS_COMPILE%-}"

# Prefer crossdev-prefix modules (built with the i586 cross-compiler on any host arch).
GRUB_MODS_SRC="/usr/${CHOST}/usr/lib/grub/i386-pc"
[ -d "$GRUB_MODS_SRC" ] || GRUB_MODS_SRC="/usr/lib/grub/i386-pc"
[ -d "$GRUB_MODS_SRC" ] || { echo "Error: GRUB i386-pc modules not found in $GRUB_MODS_SRC"; exit 1; }

mkdir -p /build/gen/boot/grub/i386-pc
cp "$GRUB_MODS_SRC"/*.mod /build/gen/boot/grub/i386-pc/

# Write GRUB configuration.
# root= is whatever board.conf declared, expanded here: it spells a
# PARTUUID built from BOOT_DISK_ID, which the pack step exports before
# this script is sourced and genimage.cfg stamps into the MBR.  Restating
# a device path here is what left BOOT_ROOT_DEV unread and the kernel
# holding a root=LABEL= it has no code to resolve.
cat > /build/gen/boot/grub/grub.cfg << EXTEOF
set timeout=3
set default=0

menuentry "Gentoo Linux (${kver})" {
    search --no-floppy --label --set=root bootfs
    linux  /${BOOT_KERNEL_NAME} root=${BOOT_ROOT_DEV} rw rootwait rootfstype=ext4 console=${BOOT_CONSOLE}
}
EXTEOF
