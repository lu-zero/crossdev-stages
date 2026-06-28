#!/bin/bash
# Zhihe A210 fastboot flash — run from a flash-bundle directory produced
# by `crossdev-stages image export --board zhihe-a210 --all`.
#
# Boot/recovery flow (vendor BOOT_SEL=000 USB fastboot):
#   1. Hold Flash button on power-on  → BootROM exposes USB fastboot
#   2. Host uploads bootzero-rvbl.bin → board DDR up
#   3. Board reboots into stage-2 fastboot
#   4. Host uploads spl-with-fit-rvbl.bin → u-boot SPL → opensbi → u-boot
#   5. u-boot fastboot accepts `flash <part> <file>` against GPT
#
# Mirrors vendor `board/zhihe/common/script/fastboot_images.sh`.
#
# Prereq: fastboot CLI on host (`emerge dev-util/android-tools`
#         or `pacman -S android-tools`).
set -e

cd "$(dirname "$(readlink -f "$0")")"

device=""
[ -n "$1" ] && device="-s $1"

FAIL="###### Image flashing failed ######"

echo "[*] checking fastboot device..."
fastboot ${device} devices

# Stage 1+2: bring up DDR + load SPL+FIT.  Skip if board reports our
# product string (already in u-boot fastboot mode from a previous run).
if fastboot ${device} getvar product 2>&1 | grep -q "product: a2"; then
    echo "[*] board already in u-boot fastboot mode — skipping bring-up"
else
    if [ -e u-boot/bootzero-rvbl.bin ]; then
        echo "[*] stage 1: bootzero-rvbl.bin (DDR init)"
        fastboot ${device} flash ram u-boot/bootzero-rvbl.bin || { echo $FAIL; exit 1; }
        fastboot ${device} reboot
        sleep 3
    fi
    echo "[*] stage 2: spl-with-fit-rvbl.bin (SPL + opensbi + u-boot)"
    fastboot ${device} flash ram u-boot/spl-with-fit-rvbl.bin || { echo $FAIL; exit 1; }
    fastboot ${device} reboot
    echo "[*] waiting for u-boot fastboot..."
    sleep 5
fi

# Stage 3: flash GPT then per-partition using the GPT-defined names.
# We don't follow vendor's A/B aliasing — flat layout, names match genimage.cfg.
#
# Vendor u-boot's `flash gpt` expects only the 17408-byte GPT structure
# (protective MBR + GPT header + entries = first 34 sectors), NOT a full
# disk image — sending the whole .img triggers `(remote: '10205000')`.
# Extract gpt.img on first run if not already present.
if [ ! -f gpt.img ]; then
    echo "[*] extracting GPT (first 34 sectors of disk image)"
    dd if=gentoo-linux-zhihe-a210_dev-emmc.img of=gpt.img bs=512 count=34 status=none
fi

echo "[*] flashing GPT (gpt.img, 17408 B)"
fastboot ${device} flash gpt gpt.img || { echo $FAIL; exit 1; }

echo "[*] flashing boot (bootfs.ext4)"
fastboot ${device} flash boot bootfs.ext4 || { echo $FAIL; exit 1; }

echo "[*] flashing rootfs (rootfs.ext4)"
fastboot ${device} flash rootfs rootfs.ext4 || { echo $FAIL; exit 1; }

echo "[*] rebooting..."
fastboot ${device} reboot

echo "###### Flash completed ######"
