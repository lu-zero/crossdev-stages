#!/bin/bash
# K3 fastboot flash — run from a flash-bundle directory produced by
# `crossdev-stages image export --board k3 --all`.
#
# K3 Pico-ITX has both NOR (SPI flash) and UFS. BROM probes NOR first.
# If a previous flash left a bootloader on NOR (e.g. Ubuntu's edk2.itb),
# our UFS flash alone will not boot — BROM will keep loading the NOR copy.
# So we flash both: NOR via partition_4M.json, UFS via partition_universal.json.
#
# Each `fastboot flash <fmt> <partition_table>.json` step registers that
# storage's partition layout in u-boot's fastboot context; subsequent
# `fastboot flash <name> <file>` then resolves names against that table.
#
# Prereq: K3 board in fastboot mode (BOOTSEL + reset),
#         fastboot CLI installed on the host (`apt install fastboot` or
#         `emerge dev-util/android-tools`).
set -e

cd "$(dirname "$(readlink -f "$0")")"

getvar() {
    fastboot getvar "$1" 2>&1 | awk -v k="$1:" '$1 == k {print $2; exit}'
}

echo "[*] checking fastboot device..."
fastboot devices

echo "[*] RAM bring-up: FSBL -> u-boot"
fastboot stage u-boot/FSBL.bin
fastboot continue
sleep 5

fastboot oem speed:super-speed || true

fastboot stage u-boot/u-boot.itb
fastboot continue
sleep 10

mtd_size=$(getvar mtd-size)
blk_size=$(getvar blk-size)
echo "[*] storage: mtd-size=${mtd_size:-none}  blk-size=${blk_size:-none}"

if [[ -n $mtd_size && $mtd_size != null ]]; then
    # vendor ships partition_4M.json; image_flash.py halves 8M -> 4M when no
    # exact match exists, and the 4M layout fits within an 8M NOR fine.
    echo "[*] flashing NOR (partition_4M.json) — clears any leftover EDK2/u-boot"
    fastboot flash mtd partition_4M.json
    fastboot flash bootinfo  u-boot/bootinfo_spinor.bin
    fastboot flash fsbl      u-boot/FSBL.bin
    fastboot flash env       u-boot/env.bin
    fastboot flash esos      esos.itb
    fastboot flash opensbi   opensbi/build/platform/generic/firmware/fw_dynamic.itb
    fastboot flash uboot     u-boot/u-boot.itb
fi

if [[ -n $blk_size && $blk_size != null ]]; then
    echo "[*] flashing UFS GPT (partition_universal.json)"
    fastboot flash gpt partition_universal.json
    fastboot flash env       u-boot/env.bin
    fastboot flash bootinfo  factory/bootinfo_block.bin
    fastboot flash fsbl      factory/FSBL.bin
    fastboot flash esos      esos.itb
    fastboot flash opensbi   opensbi/build/platform/generic/firmware/fw_dynamic.itb
    fastboot flash uboot     u-boot/u-boot.itb
    fastboot flash ESP       esp.vfat
    fastboot flash bootfs    bootfs.ext4
    fastboot flash rootfs    rootfs.ext4
fi

echo "[*] rebooting..."
fastboot reboot
