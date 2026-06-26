#!/bin/bash
set -e

# Vendor QSPI u-boot's bootflow scan picks up /extlinux/extlinux.conf
# first.  Keep mem=${ram_size}G from the vendor SDK cmdline: u-boot's
# extlinux handler expands it (ram_size set by vendor dram_init), and
# mainline-DTB boots without it die in paging_init (PMP store fault).

dtb_name=$(basename "$(ls /build/gen/boot/*.dtb | head -n1)")
[ -n "$dtb_name" ] || { echo "Error: no DTB in /build/gen/boot" >&2; exit 1; }

rm -rf /build/gen/boot/extlinux /build/gen/boot/EFI /build/gen/boot/grub

install -d /build/gen/boot/extlinux
cat > /build/gen/boot/extlinux/extlinux.conf <<EOF
default gentoo
timeout 30

label gentoo
    menu label Gentoo Linux (HiFive Premier P550)
    kernel /${BOOT_KERNEL_NAME}
    fdt /${dtb_name}
    append root=PARTLABEL=rootfs rootfstype=ext4 rootwait earlycon=sbi console=${BOOT_CONSOLE}n8 mem=\${ram_size}G
EOF
