set -e

kver=$(ls /build/gen/root/lib/modules/ | head -1)
[ -z "$kver" ] && { echo 'Error: no kernel modules found'; exit 1; }

# Derive chost from CROSS_COMPILE (strip trailing dash).
CHOST="${CROSS_COMPILE%-}"

# Prefer the crossdev-prefix modules: same portage package version as the
# sandbox grub-mkimage that built core.img, so the module ABI matches.
GRUB_MODS_SRC="/usr/${CHOST}/usr/lib/grub/i386-pc"
[ -d "$GRUB_MODS_SRC" ] || GRUB_MODS_SRC="/usr/lib/grub/i386-pc"
[ -d "$GRUB_MODS_SRC" ] || { echo "Error: GRUB i386-pc modules not found in $GRUB_MODS_SRC"; exit 1; }

# core.img was built with -p '(hd0,msdos1)/grub', so this is /grub on the
# FAT partition genimage mounts at /boot.
mkdir -p /build/gen/boot/grub/i386-pc
cp "$GRUB_MODS_SRC"/*.mod /build/gen/boot/grub/i386-pc/

# No `search --label`: core.img's baked-in prefix has already set $root to
# (hd0,msdos1), and mkfs.vfat upper-cases the volume label, so searching for
# "bootfs" is one more thing that can be quietly wrong.
#
# root= is whatever board.conf declared, expanded here.  console=tty0 first
# and serial last: this is a desktop with a monitor, and the last console=
# is the one that owns /dev/console.
cat > /build/gen/boot/grub/grub.cfg << EXTEOF
set timeout=3
set default=0

menuentry "Gentoo Linux (${kver})" {
    linux /${BOOT_KERNEL_NAME} root=${BOOT_ROOT_DEV} rw rootwait rootfstype=ext4 console=tty0 console=${BOOT_CONSOLE}
}

# Dell documents ACPI 1.0 on this machine and i386_defconfig builds an SMP
# kernel with LOCAL_APIC and IO_APIC.  If either upsets the A11 BIOS this
# entry is the way in without rewriting the image.
menuentry "Gentoo Linux (${kver}) - no ACPI, no APIC" {
    linux /${BOOT_KERNEL_NAME} root=${BOOT_ROOT_DEV} rw rootwait rootfstype=ext4 console=tty0 console=${BOOT_CONSOLE} acpi=off noapic nolapic
}
EXTEOF

# The stage3 fstab is comments only, so /boot never mounts and a kernel
# update would land in the rootfs copy instead of the partition GRUB reads.
# Same PARTUUID scheme as root=, for the same reason: it does not depend on
# the disk landing as master on the primary IDE channel.
cat >> /build/gen/root/etc/fstab <<FSTAB
PARTUUID=${BOOT_DISK_ID}-02  /      ext4  defaults,noatime  0 1
PARTUUID=${BOOT_DISK_ID}-01  /boot  vfat  defaults,noatime  0 2
FSTAB
