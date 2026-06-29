#!/bin/bash
set -e

cd "$(dirname "$(readlink -f "$0")")"

device=""
[ -n "$1" ] && device="-s $1"

FAIL="###### Image flashing failed ######"

if [ ! -f bootfs.ext4 ] && [ -f bootfs.ext4.xz ]; then
    echo "[*] decompressing bootfs.ext4..."
    unxz bootfs.ext4.xz
fi
if [ ! -f rootfs.ext4 ] && [ -f rootfs.ext4.xz ]; then
    echo "[*] decompressing rootfs.ext4..."
    unxz rootfs.ext4.xz
fi

cat <<'NOTE'

  HOLD Flash button THROUGHOUT stages 1+2 until u-boot fastboot is up.

NOTE

echo "[*] checking fastboot device..."
fastboot ${device} devices

SPL_FIT=firmware/spl-with-fit-rvbl.bin
[ -f "$SPL_FIT" ] || SPL_FIT=firmware/vendor/spl-with-fit-rvbl.bin

if fastboot ${device} getvar product 2>&1 | grep -q "product: a2"; then
    echo "[*] board already in u-boot fastboot mode — skipping bring-up"
else
    echo "[*] stage 1: bootzero-rvbl.bin"
    fastboot ${device} flash ram firmware/vendor/bootzero-rvbl.bin || { echo $FAIL; exit 1; }
    fastboot ${device} reboot

    echo "[*] stage 2: $SPL_FIT"
    fastboot ${device} flash ram "$SPL_FIT" || { echo $FAIL; exit 1; }
    fastboot ${device} reboot

    echo "[*] waiting for u-boot fastboot..."
    sleep 5
fi

echo "[*] flashing GPT"
if [ -f firmware/vendor/emmc-gpt_primary.img ]; then
    fastboot ${device} flash gpt firmware/vendor/emmc-gpt_primary.img || { echo $FAIL; exit 1; }
elif [ -f gpt.img ]; then
    fastboot ${device} flash gpt gpt.img || { echo $FAIL; exit 1; }
else
    echo "[*] extracting gpt.img"
    dd if=gentoo-linux-zhihe-a210_dev-emmc.img of=gpt.img bs=512 count=34 status=none
    fastboot ${device} flash gpt gpt.img || { echo $FAIL; exit 1; }
fi

EMMC_LOADER=firmware/emmc_boot-loader.img
[ -f "$EMMC_LOADER" ] || EMMC_LOADER=firmware/vendor/emmc_boot-loader.img
echo "[*] flashing mmc0boot0 ($EMMC_LOADER)"
fastboot ${device} flash mmc0boot0 "$EMMC_LOADER" || { echo $FAIL; exit 1; }

echo "[*] flashing uboot_env"
fastboot ${device} flash uboot_env uboot_env.img || { echo $FAIL; exit 1; }

echo "[*] flashing boot"
fastboot ${device} -S 64M flash boot bootfs.ext4 || { echo $FAIL; exit 1; }

echo "[*] flashing rootfs"
fastboot ${device} -S 64M flash rootfs rootfs.ext4 || { echo $FAIL; exit 1; }

echo "[*] rebooting..."
fastboot ${device} reboot

echo "###### Flash completed ######"
