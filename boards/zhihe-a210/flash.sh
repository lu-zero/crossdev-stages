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

# Stage 3: flash GPT then per-partition.  Names match genimage.cfg /
# vendor `gpt_emmc.txt` (`flash boot` ↔ boot_a in A-slot, etc.).
echo "[*] flashing GPT"
fastboot ${device} flash gpt gentoo-linux-zhihe-a210_dev-emmc.img || { echo $FAIL; exit 1; }

echo "[*] flashing boot (bootfs.ext4)"
fastboot ${device} flash boot_a bootfs.ext4 || { echo $FAIL; exit 1; }

echo "[*] flashing system (rootfs.ext4)"
fastboot ${device} flash system_a rootfs.ext4 || { echo $FAIL; exit 1; }

echo "[*] rebooting..."
fastboot ${device} reboot

echo "###### Flash completed ######"
