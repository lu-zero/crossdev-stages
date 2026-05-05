#!/bin/bash

# Default to K1 for backward compatibility
BOARD="k1"

# Parse command line arguments
if [[ "$1" == "--board" ]]; then
    BOARD="$2"
    shift 2
fi

# Set board-specific configuration
case "$BOARD" in
    k1)
        TAG="k1-bl-v2.2.7-release"
        OPENSBI_TAG="k1-opensbi"
        OPENSBI_REPO="https://github.com/cyyself/opensbi"
        U_BOOT_REPO="https://gitee.com/bianbu-linux/uboot-2022.10.git"
        FIRMWARE_REPO="https://gitee.com/bianbu-linux/buildroot-ext.git"
        KERNEL_REPO="https://gitee.com/bianbu-linux/linux-6.6.git"
        BOARD_CONFIG="boards/k1/board.conf"
        GENIMAGE_CFG="boards/k1/genimage.cfg"
        ;;
    k3)
        TAG="k3-br-v1.0.y"
        OPENSBI_TAG="k3-br-v1.0.y"
        OPENSBI_REPO="https://github.com/spacemit-com/opensbi"
        U_BOOT_REPO="https://github.com/spacemit-com/uboot-2022.10.git"
        FIRMWARE_REPO="https://github.com/spacemit-com/buildroot-ext.git"
        KERNEL_REPO="https://github.com/spacemit-com/linux-6.18.git"
        BOARD_CONFIG="boards/k3/board.conf"
        GENIMAGE_CFG="boards/k3/genimage.cfg"
        ;;
    *)
        echo "Error: Unknown board '$BOARD'. Supported boards: k1, k3"
        exit 1
        ;;
esac

# Override with environment variables if set
TAG="${BOARD_TAG:-$TAG}"
OPENSBI_TAG="${BOARD_OPENSBI_TAG:-$OPENSBI_TAG}"
OPENSBI_REPO="${BOARD_OPENSBI_REPO:-$OPENSBI_REPO}"
U_BOOT_REPO="${BOARD_U_BOOT_REPO:-$U_BOOT_REPO}"
FIRMWARE_REPO="${BOARD_FIRMWARE_REPO:-$FIRMWARE_REPO}"
KERNEL_REPO="${BOARD_KERNEL_REPO:-$KERNEL_REPO}"

BASE_DIR=$(dirname $(readlink -f "$0"))

usage() {
    echo "Usage: $0 [--board k1|k3] <build-directory> <stage-directory>"
    exit 1
}

if [[ -z "$2" ]]; then
    usage
fi

BUILD_DIR=$1
STAGE_DIR=$2

# Load board configuration
if [[ -f "$BASE_DIR/$BOARD_CONFIG" ]]; then
    source "$BASE_DIR/$BOARD_CONFIG"
else
    echo "Error: Board configuration not found: $BASE_DIR/$BOARD_CONFIG"
    exit 1
fi

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
        git clone --depth 1 --branch $tag $repo $src
    fi
}

checkout_all() {
    mkdir -p "$BUILD_DIR"
    checkout $OPENSBI_REPO $OPENSBI_TAG opensbi
    checkout $U_BOOT_REPO $TAG u-boot
    checkout $KERNEL_REPO $TAG linux
    checkout $FIRMWARE_REPO $TAG firmware
}

build_bootloader() {
    pushd $BUILD_DIR
    make -C opensbi PLATFORM=generic $OPENSBI_MAKE_FLAGS -j$(nproc) LLVM=1
    make -C u-boot $U_BOOT_DEFCONFIG
    make -C u-boot -j$(nproc)
    popd
}

build_linux() {
    pushd $BUILD_DIR
    make -C linux $KERNEL_DEFCONFIG
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
    make -C $BUILD_DIR/linux/tools/perf V=1 WERROR=0 DESTDIR=$(pwd)/$root/usr/ install
    mkdir -p $root/lib/firmware
    # Copy board-specific firmware
    if [[ -d "$BUILD_DIR/firmware/board/spacemit/${BOARD_NAME}/target_overlay/lib/firmware" ]]; then
        cp -a $BUILD_DIR/firmware/board/spacemit/${BOARD_NAME}/target_overlay/lib/firmware/* $root/lib/firmware/
    fi
    # Copy pre-built firmware from board directory
    if [[ -d "$BASE_DIR/boards/${BOARD_NAME}/firmware" ]]; then
        cp -a $BASE_DIR/boards/${BOARD_NAME}/firmware/* $root/lib/firmware/ 2>/dev/null || true
    fi
    # assumes we have linux-firmware installed
    cp -a /lib/firmware/rtw89 $root/lib/firmware/ 2>/dev/null || true
    mkdir -p $root/etc/dracut.conf.d
    echo 'install_items+=" /lib/firmware/esos.elf "' > $root/etc/dracut.conf.d/firmware.conf
    # Setup services from board config
    for service_runlevel in "${BOOT_SERVICES[@]}"; do
        IFS=':' read -r service runlevel <<< "$service_runlevel"
        setup_service "$service" "$runlevel"
    done
    echo "hostname=\"${BOOT_HOSTNAME}\"" > $root/etc/conf.d/hostname
    echo "${BOOT_SERIAL_TTY}:12345:respawn:/sbin/agetty ${BOOT_SERIAL_BAUD} ${BOOT_SERIAL_TTY} linux" >> $root/etc/inittab
    sed -i -e 's/root:x:/root::/' $root/etc/passwd
    echo "PermitRootLogin yes" >> $root/etc/ssh/sshd_config
    echo "PermitEmptyPasswords yes" >> $root/etc/ssh/sshd_config
    echo "StrictModes yes" >> $root/etc/ssh/sshd_config
    ldconfig -v -r $root
}

copy_to_boot() {
    local boot=$BUILD_DIR/gen/boot
    local root=$BUILD_DIR/gen/root
    mkdir -p $boot
    cp $BUILD_DIR/linux/arch/riscv/boot/$BOOT_KERNEL_NAME $boot
    cp $BUILD_DIR/linux/arch/riscv/boot/dts/spacemit/${BOARD_DTB_GLOB} $boot
    cat <<- EOF > $boot/env_${BOARD_NAME}.txt
// Common parameter
console=${BOOT_CONSOLE}
init=/init
bootdelay=0
loglevel=${BOOT_LOGLEVEL}

knl_name=${BOOT_KERNEL_NAME}
ramdisk_name=${BOOT_RAMDISK_NAME}
set_root_arg=setenv bootargs root=${BOOT_ROOT_DEV}
EOF
    DRACUT_INSTALL=/usr/lib/dracut/dracut-install \
       dracut -f --no-early-microcode --no-kernel -m "$DRACUT_MODULES" --gzip \
           --sysroot $root --tmpdir /var/tmp/ $boot/initramfs.img generic
}

generate_image() {
    pushd $BUILD_DIR
    rm -fR $BUILD_DIR/tmp
    genimage --config $BASE_DIR/$GENIMAGE_CFG
    xz -f -T0 -9 gentoo-linux-${BOARD_NAME}_dev-sdcard.img
    popd
}

# Execute build steps
for step in "${BUILD_STEPS[@]}"; do
    case $step in
        deps)    echo "Step: dependencies (handled by Docker)" ;;
        checkout) checkout_all ;;
        bootloader) build_bootloader ;;
        kernel) build_linux ;;
        assemble) copy_to_root ; copy_to_boot ;;
        pack) generate_image ;;
        *) echo "Unknown build step: $step" ; exit 1 ;;
    esac
done
