# SpacemiT KY-X1 (OrangePi RV2)

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
    cp $BUILD_DIR/linux/arch/riscv/boot/Image.itb $boot
    cp $BUILD_DIR/linux/arch/riscv/boot/dts/ky/*.dtb $boot
    DRACUT_INSTALL=/usr/lib/dracut/dracut-install \
       dracut -f --no-early-microcode --no-kernel -m "busybox" \
           --sysroot $root --tmpdir /var/tmp/ $boot/initramfs.img generic
    mkimage -A riscv -T script -C none -d $BOARD_DIR/boot.cmd $boot/boot.scr
    mkimage -A riscv -O linux -T ramdisk -C gzip -d $boot/initramfs.img $boot/uInitrd
}
