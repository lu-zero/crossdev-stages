set -e

# DTBs into nested directory
mkdir -p /build/gen/boot/dtbs
cp /build/linux/${BOARD_DTB_GLOB} /build/gen/boot/dtbs/

# Extlinux boot config
mkdir -p /build/gen/boot/extlinux
kver=$(ls /build/gen/root/lib/modules/ | head -1)
[ -z "$kver" ] && { echo 'Error: no kernel modules found'; exit 1; }
cat > /build/gen/boot/extlinux/extlinux.conf << EXTEOF
DEFAULT canmv
TIMEOUT 30

LABEL canmv
    MENU LABEL CanMV K230 V1.1 (512MB)
    LINUX /vmlinuz-$kver
    FDT /dtbs/k230_canmv.dtb
    APPEND root=${BOOT_ROOT_DEV} rw rootwait rootfstype=ext4 console=${BOOT_CONSOLE} earlycon=sbi

LABEL 01studio
    MENU LABEL 01Studio CanMV K230 (1GB)
    LINUX /vmlinuz-$kver
    FDT /dtbs/k230_canmv_01studio.dtb
    APPEND root=${BOOT_ROOT_DEV} rw rootwait rootfstype=ext4 console=${BOOT_CONSOLE} earlycon=sbi
EXTEOF

# K230 firmware header for u-boot SPL
python3 /scripts/boards/k230/make-k230-firmware.py \
    -i /build/u-boot/spl/u-boot-spl.bin \
    -o /build/u-boot/fn_u-boot-spl.bin

# Package u-boot as gzipped uImage
gzip -fkn9 /build/u-boot/u-boot.bin
mkimage -A riscv -C gzip -O u-boot -T firmware -a 0 -e 0 -n uboot \
    -d /build/u-boot/u-boot.bin.gz /build/u-boot/ug_u-boot.bin

# Package opensbi+kernel as gzipped vmlinuz
opensbi=/build/opensbi/build/platform/${OPENSBI_PLATFORM}/firmware/fw_payload.bin
gzip -fkn9 $opensbi
mkimage -A riscv -C gzip -O linux -T kernel -a 0 -e 0 -n linux \
    -d $opensbi.gz /build/gen/boot/vmlinuz-$kver
