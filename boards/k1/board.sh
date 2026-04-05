# SpacemiT K1 (BPI-F3, Milk-V Jupiter, DC Roma II)
# Board-specific assembly: firmware overlay, boot image, initramfs

board_assemble() {
    local sandbox_dir="$1"
    local build_dir="$2"
    local source_dir="$3"

    # Build extra bind args for host firmware paths
    local extra_args=()
    for fw_path in "${HOST_FIRMWARE_PATHS[@]+"${HOST_FIRMWARE_PATHS[@]}"}"; do
        [[ -d "$fw_path" ]] && extra_args+=("-b" "${fw_path}:${fw_path}")
    done

    # Pre-compute host firmware copy commands
    local fw_cmds=""
    for fw_path in "${HOST_FIRMWARE_PATHS[@]+"${HOST_FIRMWARE_PATHS[@]}"}"; do
        fw_cmds+="cp -a '${fw_path}' /build/gen/root/lib/firmware/ 2>/dev/null || true; "
    done

    run_with_build_and_source "$sandbox_dir" "$build_dir" "$source_dir" \
      "${extra_args[@]}" -- "
        set -e

        # Copy DTBs
        cp /build/linux/${BOARD_DTB_GLOB} /build/gen/boot/

        # Board firmware overlay
        mkdir -p /build/gen/root/lib/firmware
        cp -a /build/firmware/${BOARD_FIRMWARE_OVERLAY}/. /build/gen/root/lib/firmware/

        # Host firmware (wifi, etc.)
        ${fw_cmds}

        # Dracut firmware hint
        mkdir -p /build/gen/root/etc/dracut.conf.d
        echo 'install_items+=\" /lib/firmware/esos.elf \"' > /build/gen/root/etc/dracut.conf.d/firmware.conf

        # Copy kernel image
        cp /build/linux/arch/${KERNEL_ARCH}/boot/${BOOT_KERNEL_NAME} /build/gen/boot/

        # U-Boot environment
        printf 'console=${BOOT_CONSOLE}\ninit=/init\nbootdelay=0\nloglevel=${BOOT_LOGLEVEL}\nknl_name=${BOOT_KERNEL_NAME}\nramdisk_name=${BOOT_RAMDISK_NAME}\nset_root_arg=setenv bootargs root=${BOOT_ROOT_DEV}\n' \
            > /build/gen/boot/env_${BOARD_NAME}-x.txt

        # Build initramfs
        kver=\$(ls /build/gen/root/lib/modules/ | head -1)
        dracutbasedir=/usr/lib/dracut \
        DRACUT_INSTALL=/usr/lib/dracut/dracut-install \
          dracut -f --no-early-microcode --no-kernel \
            -m '${DRACUT_MODULES}' --gzip \
            --sysroot /build/gen/root \
            --tmpdir /tmp \
            /build/gen/boot/initramfs.img \"\$kver\"
    "
}
