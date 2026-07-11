#!/bin/bash
# Zhihe A210 fastboot flash — run from a flash-bundle directory produced
# by `crossdev-stages image export --board zhihe-a210 --all`, or straight
# from the per-board build directory (artifacts in u-boot/).
set -e

cd "$(dirname "$(readlink -f "$0")")"

device=()
[ -n "$1" ] && device=(-s "$1")

FAIL="###### Image flashing failed ######"

# Probe the known artifact locations: bundle root (export --all), build
# dir u-boot/ subdir, and manual drops in firmware/[vendor/].
find_art() {
    local d
    for d in . u-boot firmware firmware/vendor; do
        [ -f "$d/$1" ] && { echo "$d/$1"; return 0; }
    done
    return 1
}

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
fastboot "${device[@]}" devices

if fastboot "${device[@]}" getvar product 2>&1 | grep -q "product: a2"; then
    echo "[*] board already in u-boot fastboot mode — skipping bring-up"
else
    BOOTZERO=$(find_art bootzero-rvbl.bin) || {
        echo "ERROR: bootzero-rvbl.bin not found (vendor blob, see firmware/README.md)"
        echo "$FAIL"; exit 1
    }
    SPL_FIT=$(find_art spl-with-fit-rvbl.bin) || {
        echo "ERROR: spl-with-fit-rvbl.bin not found (built by post-assemble.sh)"
        echo "$FAIL"; exit 1
    }

    echo "[*] stage 1: $BOOTZERO"
    fastboot "${device[@]}" flash ram "$BOOTZERO" || { echo "$FAIL"; exit 1; }
    fastboot "${device[@]}" reboot

    echo "[*] stage 2: $SPL_FIT"
    fastboot "${device[@]}" flash ram "$SPL_FIT" || { echo "$FAIL"; exit 1; }
    fastboot "${device[@]}" reboot

    echo "[*] waiting for u-boot fastboot..."
    sleep 5
fi

echo "[*] flashing GPT"
if GPT_IMG=$(find_art emmc-gpt_primary.img); then
    fastboot "${device[@]}" flash gpt "$GPT_IMG" || { echo "$FAIL"; exit 1; }
elif [ -f gpt.img ]; then
    fastboot "${device[@]}" flash gpt gpt.img || { echo "$FAIL"; exit 1; }
else
    echo "[*] extracting gpt.img"
    dd if=gentoo-linux-zhihe-a210_dev-emmc.img of=gpt.img bs=512 count=34 status=none
    fastboot "${device[@]}" flash gpt gpt.img || { echo "$FAIL"; exit 1; }
fi

if EMMC_LOADER=$(find_art emmc_boot-loader.img); then
    echo "[*] flashing mmc0boot0 ($EMMC_LOADER)"
    fastboot "${device[@]}" flash mmc0boot0 "$EMMC_LOADER" || { echo "$FAIL"; exit 1; }
else
    echo "[*] WARN: emmc_boot-loader.img not found — skipping mmc0boot0"
    echo "    (built only when bootzero2.bin is present at build time; eMMC"
    echo "     boot needs a loader in mmc0boot0 — see firmware/README.md)"
fi

if UBOOT_ENV=$(find_art uboot_env.img); then
    echo "[*] flashing uboot_env ($UBOOT_ENV)"
    fastboot "${device[@]}" flash uboot_env "$UBOOT_ENV" || { echo "$FAIL"; exit 1; }
else
    echo "[*] WARN: uboot_env.img not found — skipping uboot_env"
    echo "    (set bootcmd/bootargs manually over serial — see README.md)"
fi

echo "[*] flashing boot"
fastboot "${device[@]}" -S 64M flash boot bootfs.ext4 || { echo "$FAIL"; exit 1; }

echo "[*] flashing rootfs"
fastboot "${device[@]}" -S 64M flash rootfs rootfs.ext4 || { echo "$FAIL"; exit 1; }

echo "[*] rebooting..."
fastboot "${device[@]}" reboot

echo "###### Flash completed ######"
