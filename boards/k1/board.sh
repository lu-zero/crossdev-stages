# SpacemiT K1 (BPI-F3, Milk-V Jupiter, DC Roma II)

board_root() {
    default_root
    local root=$BUILD_DIR/gen/root
    mkdir -p $root/lib/firmware
    cp -a $BUILD_DIR/firmware/board/spacemit/k1/target_overlay/lib/firmware/* $root/lib/firmware
    cp -a /lib/firmware/rtw89 $root/lib/firmware/
    mkdir -p $root/etc/dracut.conf.d
    echo 'install_items+=" /lib/firmware/esos.elf "' > $root/etc/dracut.conf.d/firmware.conf
}

board_boot() {
    local boot=$BUILD_DIR/gen/boot
    local root=$BUILD_DIR/gen/root
    mkdir -p $boot
    cp $BUILD_DIR/linux/arch/riscv/boot/Image.gz.itb $boot
    cp $BUILD_DIR/linux/arch/riscv/boot/dts/spacemit/*.dtb $boot
    cat <<- EOF > $boot/env_k1-x.txt
// Common parameter
console=ttyS0,115200
init=/init
bootdelay=0
loglevel=8

knl_name=Image.gz.itb
ramdisk_name=initramfs.img
// Workaround bogus UUID computation
set_root_arg=setenv bootargs  root=/dev/mmcblk0p6
EOF
    DRACUT_INSTALL=/usr/lib/dracut/dracut-install \
       dracut -f --no-early-microcode --no-kernel -m "busybox" --gzip \
           --sysroot $root --tmpdir /var/tmp/ $boot/initramfs.img generic
}
