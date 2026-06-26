#!/bin/bash
set -e

# Vendor QSPI u-boot 2024.01-EIC7X runs `bootflow scan -b` over
# boot_targets = mmc1 usb nvme0 ahci mmc0.  Bootmeth priority is hard-
# coded by the u-boot build:
#   1.  /extlinux/extlinux.conf  +  /boot/extlinux/extlinux.conf
#   2.  /boot.scr.uimg           +  /boot.scr
#   3.  /EFI/BOOT/bootriscv64.efi    (EFI removable-media fallback)
#
# We DON'T ship extlinux.conf — we want GRUB primary (interactive menu,
# matches RHEL10 / Ubuntu / Fedora distro flow, easier to edit on
# rescue media).  Skipping extlinux makes u-boot fall through to #3
# and hand control to grub-mkimage'd bootriscv64.efi.

dtb_name=$(basename "$(ls /build/gen/boot/*.dtb | head -n1)")
kver=$(ls /build/gen/root/lib/modules/ | head -n1)

# Clean stale paths from prior builds that might have shipped extlinux
# or a different bootloader layout.
rm -rf /build/gen/boot/extlinux /build/gen/boot/EFI /build/gen/boot/grub

# riscv64-efi modules come from the crossdev prefix (built by sandbox.rs
# via `crossdev --ex-pkg sys-boot/grub` triggered by GRUB_PLATFORMS=
# riscv64-efi in board.conf).
CHOST="${CROSS_COMPILE%-}"
GRUB_MODS_SRC="/usr/${CHOST}/usr/lib/grub/riscv64-efi"
[ -d "$GRUB_MODS_SRC" ] || {
    echo "Error: riscv64-efi grub modules not found at $GRUB_MODS_SRC"
    exit 1
}

# Embed enough modules in the .efi to read grub.cfg off FAT/ext2 and
# chain into the kernel with a DTB.  Anything else loads at runtime from
# /grub/riscv64-efi/.  Note: riscv64-efi DT support module is `fdt` (x86
# uses `devicetree`); the in-config command name stays `devicetree`.
GRUB_EMBED_MODS="part_gpt part_msdos fat ext2 normal boot linux configfile \
    echo search search_label search_fs_uuid fdt efi_gop all_video terminal"

install -d /build/gen/boot/EFI/BOOT
grub-mkimage -O riscv64-efi \
    -d "$GRUB_MODS_SRC" \
    -o /build/gen/boot/EFI/BOOT/bootriscv64.efi \
    -p '/grub' \
    ${GRUB_EMBED_MODS}

# Stage runtime-loadable modules — small enough not to hurt.
install -d /build/gen/boot/grub/riscv64-efi
cp "$GRUB_MODS_SRC"/*.mod /build/gen/boot/grub/riscv64-efi/

# grub.cfg — NO `mem=…` cmdline.  Vendor u-boot doesn't expand
# ${ram_size}; even if a user wrote one, leaving it literal makes the
# kernel see `mem=${ram_size}G`, fail to parse, "Memory limited to
# 0MB", and Oops in paging_init's __memset.  The EIC7700 DTB /memory
# node advertises the correct DRAM range (16 GiB effective) already.
install -d /build/gen/boot/grub
cat > /build/gen/boot/grub/grub.cfg <<EOF
set timeout=3
set default=0

menuentry "Gentoo Linux ${kver}" {
    search --no-floppy --label --set=root BOOTFS
    linux /Image root=PARTLABEL=rootfs rootfstype=ext4 rootwait earlycon=sbi console=${BOOT_CONSOLE}n8
    devicetree /${dtb_name}
}
EOF
