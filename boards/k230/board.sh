# Canaan K230 (CanMV-K230-V1.1, 01Studio CanMV K230)
#
# K230 build order differs from the default: kernel must be built before
# opensbi because opensbi embeds the kernel Image as FW_PAYLOAD.

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
        checkout '${U_BOOT_REPO}' '${U_BOOT_TAG}' u-boot
        checkout '${KERNEL_REPO}' '${KERNEL_TAG}' linux
    "
}

board_build_kernel() {
    local sandbox_dir="$1"
    local build_dir="$2"

    run_with_build "$sandbox_dir" "$build_dir" "
        make -C /build/linux ARCH=${KERNEL_ARCH} CROSS_COMPILE=${CROSS_COMPILE} ${KERNEL_DEFCONFIG}
        make -C /build/linux ARCH=${KERNEL_ARCH} CROSS_COMPILE=${CROSS_COMPILE} -j\$(nproc)
        make -C /build/linux ARCH=${KERNEL_ARCH} CROSS_COMPILE=${CROSS_COMPILE} modules -j\$(nproc)
        make -C /build/linux ARCH=${KERNEL_ARCH} CROSS_COMPILE=${CROSS_COMPILE} dtbs
    "
}

board_build_bootloader() {
    local sandbox_dir="$1"
    local build_dir="$2"

    # opensbi embeds kernel Image as FW_PAYLOAD (BUILD_STEPS runs kernel before bootloader)
    run_with_build "$sandbox_dir" "$build_dir" "
        make -C /build/opensbi PLATFORM=${OPENSBI_PLATFORM} CROSS_COMPILE=${CROSS_COMPILE} \
            FW_PAYLOAD=y FW_PAYLOAD_PATH=/build/linux/arch/${KERNEL_ARCH}/boot/Image -j\$(nproc)
        make -C /build/u-boot ARCH=${KERNEL_ARCH} CROSS_COMPILE=${CROSS_COMPILE} ${U_BOOT_DEFCONFIG}
        make -C /build/u-boot ARCH=${KERNEL_ARCH} CROSS_COMPILE=${CROSS_COMPILE} -j\$(nproc)
    "
}

board_assemble() {
    local sandbox_dir="$1"
    local build_dir="$2"
    local source_dir="$3"

    run_with_build_and_source "$sandbox_dir" "$build_dir" "$source_dir" -- "
        set -e

        # Copy DTBs
        mkdir -p /build/gen/boot/dtbs
        cp /build/linux/${BOARD_DTB_GLOB} /build/gen/boot/dtbs/

        # Extlinux boot config
        mkdir -p /build/gen/boot/extlinux
        kver=\$(ls /build/gen/root/lib/modules/ | head -1)
        cat > /build/gen/boot/extlinux/extlinux.conf << EXTEOF
DEFAULT canmv
TIMEOUT 30

LABEL canmv
    MENU LABEL CanMV K230 V1.1 (512MB)
    LINUX /vmlinuz-\$kver
    FDT /dtbs/k230_canmv.dtb
    APPEND root=${BOOT_ROOT_DEV} rw rootwait rootfstype=ext4 console=${BOOT_CONSOLE} earlycon=sbi

LABEL 01studio
    MENU LABEL 01Studio CanMV K230 (1GB)
    LINUX /vmlinuz-\$kver
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
        gzip -fkn9 \$opensbi
        mkimage -A riscv -C gzip -O linux -T kernel -a 0 -e 0 -n linux \
            -d \$opensbi.gz /build/gen/boot/vmlinuz-\$kver
    "
}
