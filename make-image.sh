#!/bin/bash

# Make-image script - refactored to use external configuration

# Source common library
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/lib/common.sh"

# Default configuration
DEFAULT_PLATFORM="riscv64-k1"
DEFAULT_CONFIG="config/platforms/${DEFAULT_PLATFORM}.conf"

# Help function
display_help() {
    echo "Usage: $0 [options] <build-directory> <stage-directory>"
    echo
    echo "Options:"
    echo "  --config,-c <file>  Use alternative configuration file"
    echo "  --platform,-p <name> Use specific platform configuration"
    echo "  --help,-h           Show this help message"
    echo
    echo "This script builds a complete bootable image for the target platform."
    echo "It checks out sources, builds bootloader and kernel, and assembles the final image."
    echo
    echo "Examples:"
    echo "  $0 --help                          Show this help"
    echo "  $0 /path/to/build /path/to/stage   Build image with default config"
    echo "  $0 --platform riscv64-k1 /build /stage"
    exit 0
}

# Parse command line arguments (but preserve non-option arguments)
TEMP_ARGS=()
while [[ $# -gt 0 ]]; do
    case $1 in
        --help|-h)
            display_help
            exit 0
            ;;
        --config|-c)
            CONFIG_FILE="$2"
            shift 2
            ;;
        --platform|-p)
            PLATFORM="$2"
            if [[ -n "$PLATFORM" ]]; then
                CONFIG_FILE="config/platforms/${PLATFORM}.conf"
            fi
            shift 2
            ;;
        --verbose|-v)
            VERBOSE=1
            shift
            ;;
        --*)
            echo "Unknown option: $1"
            usage
            exit 1
            ;;
        *)
            TEMP_ARGS+=("$1")
            shift
            ;;
    esac
done

# Restore non-option arguments
set -- "${TEMP_ARGS[@]}"

# Load configuration
CONFIG_FILE="${CONFIG_FILE:-$DEFAULT_CONFIG}"
load_config "$CONFIG_FILE"

# Set global variables from config
BUILD_DIR=$1
STAGE_DIR=$2

export CROSS_COMPILE="${TARGET_CHOST}-"
export OPENSBI="$BUILD_DIR"/opensbi/build/platform/generic/firmware/fw_dynamic.bin
export ARCH=riscv

checkout() {
    checkout_repo "$1" "$2" "$BUILD_DIR/$3"
}

checkout_all() {
    mkdir -p "$BUILD_DIR"
    checkout "${OPENSBI_REPO}" "${OPENSBI_TAG}" opensbi
    checkout "${U_BOOT_REPO}" "${BOOTLOADER_TAG}" u-boot
    checkout "${KERNEL_REPO}" "${BOOTLOADER_TAG}" linux
    checkout "${FIRMWARE_REPO}" "${BOOTLOADER_TAG}" firmware
}

build_bootloader() {
    pushd $BUILD_DIR
    make -C opensbi PLATFORM=generic PLATFORM_DEFCONFIG=defconfig -j$(nproc) LLVM=1
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
    mkdir -p "$root"
    cp -a "$STAGE_DIR"/* "$root"
    INSTALL_MOD_PATH="$root" make -C "$BUILD_DIR/linux" modules_install
    make -C "$BUILD_DIR/linux/tools/perf" V=1 WERROR=0 DESTDIR="$(pwd)/$root/usr/" install
    mkdir -p "$root/lib/firmware"
    cp -a "$BUILD_DIR/firmware/board/spacemit/k1/target_overlay/lib/firmware"/* "$root/lib/firmware"
    # assumes we have linux-firmware installed
    cp -a /lib/firmware/rtw89 "$root/lib/firmware/"
    mkdir -p "$root/etc/dracut.conf.d"
    echo 'install_items+=" /lib/firmware/esos.elf "' > "$root/etc/dracut.conf.d/firmware.conf"
    setup_service sshd default
    setup_service metalog default
    setup_service ntp-client default
    echo 'hostname="gentoo"' > "$root/etc/conf.d/hostname"
    echo "x1:12345:respawn:/sbin/agetty 115200 console linux" >> "$root/etc/inittab"
    sed -i -e 's/root:x:/root::/' "$root/etc/passwd"
    echo "PermitRootLogin yes" >> "$root/etc/ssh/sshd_config"
    echo "PermitEmptyPasswords yes" >> "$root/etc/ssh/sshd_config"
    echo "StrictModes yes" >> "$root/etc/ssh/sshd_config"
    ldconfig -v -r "$root"
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
// Workaround bogus UUID computation
set_root_arg=setenv bootargs  root=/dev/mmcblk0p6
EOF
    DRACUT_INSTALL=/usr/lib/dracut/dracut-install \
       dracut -f --no-early-microcode --no-kernel -m "busybox" --gzip \
           --sysroot $root --tmpdir /var/tmp/ $boot/initramfs.img generic
}

generate_image() {
    pushd "$BUILD_DIR"
    rm -fR "$BUILD_DIR/tmp"
    genimage --config "${GENIMAGE_CONFIG}"
    xz -f -T0 -9 gentoo-linux-k1_dev-sdcard.img
    popd
}

# Check arguments
if [[ -z "$2" ]]; then
    display_help
fi

# Main execution
checkout_all
build_bootloader
build_linux
copy_to_root
copy_to_boot
generate_image
