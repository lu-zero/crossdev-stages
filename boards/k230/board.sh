# Canaan K230 (CanMV-K230-V1.1, 01Studio CanMV K230)

board_boot() {
    local boot=$BUILD_DIR/gen/boot
    mkdir -p $boot/dtbs
    cp $BUILD_DIR/linux/arch/riscv/boot/dts/canaan/k230_canmv.dtb $boot/dtbs
    cp $BUILD_DIR/linux/arch/riscv/boot/dts/canaan/k230_canmv_01studio.dtb $boot/dtbs
    mkdir -p $boot/extlinux
    cat > $boot/extlinux/extlinux.conf << 'EOF'
DEFAULT canmv
TIMEOUT 30

LABEL canmv
    MENU LABEL CanMV K230 V1.1 (512MB)
    LINUX /vmlinuz-6.12.3
    FDT /dtbs/k230_canmv.dtb
    APPEND root=/dev/mmcblk1p2 rw rootwait rootfstype=ext4 console=ttyS0,115200n8 earlycon=sbi

LABEL 01studio
    MENU LABEL 01Studio CanMV K230 (1GB)
    LINUX /vmlinuz-6.12.3
    FDT /dtbs/k230_canmv_01studio.dtb
    APPEND root=/dev/mmcblk1p2 rw rootwait rootfstype=ext4 console=ttyS0,115200n8 earlycon=sbi
EOF
    python $BOARD_DIR/make-k230-firmware.py -i $BUILD_DIR/u-boot/spl/u-boot-spl.bin -o $BUILD_DIR/u-boot/fn_u-boot-spl.bin
    gzip -fkn9 $BUILD_DIR/u-boot/u-boot.bin
    mkimage -A riscv -C gzip -O u-boot -T firmware -a 0 -e 0 -n uboot \
        -d $BUILD_DIR/u-boot/u-boot.bin.gz $BUILD_DIR/u-boot/ug_u-boot.bin
    gzip -fkn9 $OPENSBI
    mkimage -A riscv -C gzip -O linux -T kernel -a 0 -e 0 -n linux \
        -d $OPENSBI.gz $boot/vmlinuz-6.12.3
}
