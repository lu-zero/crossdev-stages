# Tenstorrent Blackhole P100/P150 (SiFive X280)
#
# PCIe card - no SD card, no u-boot.
# Host tool loads opensbi+kernel+dtb directly into DRAM via PCIe BAR.
# We only need to produce a rootfs.ext4 image.

board_checkout() {
    local sandbox_dir="$1"
    local build_dir="$2"

    run_with_build "$sandbox_dir" "$build_dir" "
        checkout() {
            local repo=\$1 tag=\$2 src=/build/\$3
            if [[ -d \"\$src\" ]]; then
                (cd \"\$src\" && git fetch && git checkout \"\$tag\")
            else
                git clone --depth 1 --branch \"\$tag\" \"\$repo\" \"\$src\"
            fi
        }
        checkout '${OPENSBI_REPO}' '${OPENSBI_TAG}' opensbi
        checkout '${KERNEL_REPO}' '${KERNEL_TAG}' linux
    "
}

board_build_bootloader() {
    local sandbox_dir="$1"
    local build_dir="$2"

    # opensbi fw_jump (no u-boot)
    run_with_build "$sandbox_dir" "$build_dir" "
        make -C /build/opensbi PLATFORM=${OPENSBI_PLATFORM} CROSS_COMPILE=${CROSS_COMPILE} FW_JUMP=y -j\$(nproc)
    "
}

board_assemble() {
    local sandbox_dir="$1"
    local build_dir="$2"
    local source_dir="$3"

    run_with_build_and_source "$sandbox_dir" "$build_dir" "$source_dir" -- "
        set -e

        # Install kernel modules
        INSTALL_MOD_PATH=/build/gen/root make -C /build/linux ARCH=${KERNEL_ARCH} CROSS_COMPILE=${CROSS_COMPILE} modules_install

        # Copy kernel Image and DTB to build dir (for host tool)
        cp /build/linux/arch/${KERNEL_ARCH}/boot/Image /build/
        cp /build/linux/arch/${KERNEL_ARCH}/boot/dts/tenstorrent/*.dtb /build/ 2>/dev/null || true

        # Copy opensbi fw_jump
        cp /build/opensbi/build/platform/${OPENSBI_PLATFORM}/firmware/fw_jump.bin /build/

        # Update ldconfig
        ${LDCONFIG} -v -r /build/gen/root
    "
}
