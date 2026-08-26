set -e

# Exynos 5422 SD boot region, taken from Hardkernel's sd_fuse/sd_fusing.sh
# (512-byte sectors, dd's default block size):
#
#   sector    1  BL1                 15360 B, Samsung-signed, verified by iROM
#   sector   31  BL2                 14592 B, Samsung-signed, brings up DRAM
#   sector   63  U-Boot             720 KiB window
#   sector 1503  TrustZone monitor  262144 B, Samsung-signed
#   sector 6272  U-Boot environment  16 KiB (CONFIG_ENV_OFFSET=0x310000)
#
# The BL2 variant fixes that U-Boot window.  This one ("720k_uboot") loads
# exactly 720 KiB from sector 63 and jumps to the monitor at sector 1503;
# Hardkernel's older BL2 uses 328 KiB and sector 719 instead.  Moving one
# without the other gives a board that goes quiet after BL2.
#
# Sectors 1..2014 are assembled into one flat file so genimage writes a single
# raw region and never has to reason about the sub-sector layout.

blobs=/build/exynos-blobs/sd_fuse
out=/build/exynos
img="${out}/sd-boot.bin"

# CONFIG_OF_SEPARATE keeps the control DTB out of u-boot.bin, so the blob that
# BL2 loads has to be the one with the DTB appended.
if [ -f /build/u-boot/u-boot-dtb.bin ]; then
    uboot=/build/u-boot/u-boot-dtb.bin
else
    uboot=/build/u-boot/u-boot.bin
fi
[ -f "${uboot}" ] || { echo "Error: no U-Boot binary at ${uboot}"; exit 1; }

window=$((720 * 1024))
size=$(stat -c%s "${uboot}")
if [ "${size}" -gt "${window}" ]; then
    echo "Error: ${uboot} is ${size} bytes, over the ${window} byte window BL2 loads."
    echo "Drop what SD boot does not need from odroid-xu3_defconfig"
    echo "(CMD_THOR_DOWNLOAD, CMD_DFU, USB_GADGET) or use a BL2 with a wider window."
    exit 1
fi

rm -rf "${out}"
mkdir -p "${out}"

# 2014 sectors: sector 1 through the last TrustZone sector, stopping short of
# the environment so a stale environment is never carried into a fresh image.
dd if=/dev/zero of="${img}" bs=512 count=2014 status=none

# seek= is relative to sector 1, i.e. one less than the absolute sector above.
#
# BL1 on this branch is 15616 bytes, but only the first 15360 are BL1: the
# trailing 256 bytes land on BL2's first sector and Hardkernel's own script
# overwrites them a moment later.  The odroidxu3-v2012.07 branch ships that
# same BL1 as exactly 15360 bytes, byte for byte, so take 30 sectors and skip
# the overlap that genimage would refuse to write.
dd if="${blobs}/bl1.bin.hardkernel" of="${img}" \
   bs=512 count=30 conv=notrunc status=none
dd if="${blobs}/bl2.bin.hardkernel.720k_uboot" of="${img}" \
   bs=512 seek=30 conv=notrunc status=none
dd if="${uboot}" of="${img}" \
   bs=512 seek=62 conv=notrunc status=none
dd if="${blobs}/tzsw.bin.hardkernel" of="${img}" \
   bs=512 seek=1502 conv=notrunc status=none

echo "Exynos boot region assembled: ${img} (U-Boot ${size}/${window} bytes)"
