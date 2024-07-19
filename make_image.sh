#!/bin/bash

BOOT_TAG="v1.0.3"
OPENSBI_REPO="https://github.com/lu-zero/pi-opensbi"
U_BOOT_REPO="https://github.com/lu-zero/pi-u-boot"
KERNEL_TAG="v1.0.3-lu"
KERNEL_REPO="https://github.com/lu-zero/pi-linux"
REMOTEPROC_FIRMWARE="https://github.com/BPI-SINOVOIP/armbian-build/raw/f4d657eda0400386bb2bf6d4db8798741afae963/packages/bsp/bananapif3/root/usr/lib/firmware/esos.elf"

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
    mkdir -p $BUILD_DIR/firmware
    curl $REMOTEPROC_FIRMWARE > $BUILD_DIR/firmware/esos.elf
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

copy_to_boot() {
    local boot=$BUILD_DIR/gen/boot
    mkdir -p $boot
    cp $BUILD_DIR/linux/arch/riscv/boot/Image.gz.itb $boot
    cp $BUILD_DIR/linux/arch/riscv/boot/dts/spacemit/*.dts $boot
    cat <<- EOF > $boot/env_k1-x.txt
// Common parameter
console=ttyS0,115200
init=/init
bootdelay=0
loglevel=8
EOF
}

copy_to_root() {
    local root=$BUILD_DIR/gen/root
    mkdir -p $root
    cp -a $STAGE_DIR/* $root
    INSTALL_MOD_PATH=$root make -C $BUILD_DIR/linux modules_install

}

checkout_all
build_bootloader
build_linux
copy_to_boot
copy_to_root

