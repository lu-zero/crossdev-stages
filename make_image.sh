#!/bin/bash

BOOT_TAG="v1.0.3"
OPENSBI_REPO="https://github.com/lu-zero/pi-opensbi"
U_BOOT_REPO="https://github.com/lu-zero/pi-u-boot"
FIRMWARE_REPO="https://gitee.com/bianbu-linux/buildroot-ext.git"
KERNEL_TAG="v1.0.3-lu"
KERNEL_REPO="https://github.com/lu-zero/pi-linux"

BASE_DIR=$(dirname $(readlink -f "$0"))

usage() {
    echo "Usage: $0 <build-directory> <stage-directory>"
    exit 1
}

if [[ -z "$2" ]]; then
    usage
fi

BUILD_DIR=$1
STAGE_DIR=$2

export CROSS_COMPILE=riscv64-unknown-linux-gnu-
export OPENSBI="$BUILD_DIR"/opensbi/build/platform/generic/firmware/fw_dynamic.bin
export ARCH=riscv

checkout() {
    local repo=$1
    local tag=$2
    local src=$BUILD_DIR/$3
    if [[ -d $src ]]; then
        (cd $src && git fetch && git checkout "$tag")
    else
        git clone --branch $tag $repo $src
    fi
}

checkout_all() {
    mkdir -p "$BUILD_DIR"
    checkout $OPENSBI_REPO $BOOT_TAG opensbi
    checkout $U_BOOT_REPO $BOOT_TAG u-boot
    checkout $KERNEL_REPO $KERNEL_TAG linux
    checkout $FIRMWARE_REPO $BOOT_TAG firmware
}

build_bootloader() {
    pushd $BUILD_DIR
    make -C opensbi PLATFORM=generic PLATFORM_DEFCONFIG=k1_defconfig -j$(nproc)
    make -C u-boot k1_defconfig
    make -C u-boot -j$(nproc)
    popd
}

build_linux() {
    pushd $BUILD_DIR
    make -C linux k1_defconfig
    make -C linux -j$(nproc)
    make -C linux modules -j$(nproc)
    popd
}

setup_service() {
    local service=$1
    local runlevel=$2
    local root=$BUILD_DIR/gen/root
    ln -sf /etc/init.d/$1 $root/etc/runlevels/$2/
}

copy_to_root() {
    local root=$BUILD_DIR/gen/root
    mkdir -p $root
    cp -a $STAGE_DIR/* $root
    INSTALL_MOD_PATH=$root make -C $BUILD_DIR/linux modules_install
    mkdir -p $root/lib/firmware
    cp -a $BUILD_DIR/firmware/board/spacemit/k1/target_overlay/lib/firmware/* $root/lib/firmware
    mkdir -p $root/etc/dracut.conf.d
    echo 'install_items+=" /lib/firmware/esos.elf "' > $root/etc/dracut.conf.d/firmware.conf
    setup_service sshd default
    setup_service metalog default
    echo "x1:12345:respawn:/sbin/agetty 115200 console linux" >> $root/etc/inittab
    sed -i -e 's/root:x:/root::/' $root/etc/passwd
    echo "PermitRootLogin yes" >> $root/etc/ssh/sshd_config
    echo "PermitEmptyPasswords yes" >> $root/etc/ssh/sshd_config
    echo "StrictModes yes" >> $root/etc/ssh/sshd_config
}

copy_to_boot() {
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
EOF
    DRACUT_INSTALL=/usr/lib/dracut/dracut-install \
       dracut -f --no-early-microcode --no-kernel -m "busybox" --gzip \
           --sysroot $root --tmpdir /var/tmp/ $boot/initramfs.img generic
}

generate_image() {
    pushd $BUILD_DIR
    rm -fR $BUILD_DIR/tmp
    genimage --config $BASE_DIR/genimage.cfg
    xz -f -T0 -9 gentoo-linux-k1_dev-sdcard.img 
    popd
}

checkout_all
build_bootloader
build_linux
copy_to_root
copy_to_boot
generate_image
