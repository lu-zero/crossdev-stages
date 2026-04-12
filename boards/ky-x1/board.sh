# SpacemiT KY-X1 (OrangePi RV2)
#
# Same chipset as K1 but different repos and boot method (boot.scr + uInitrd).

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
        checkout '${FIRMWARE_REPO}' '${TAG}' firmware
    "
}

board_assemble() {
    local sandbox_dir="$1"
    local build_dir="$2"
    local source_dir="$3"

    # Build extra bind args for host firmware paths
    local extra_args=()
    for fw_path in "${HOST_FIRMWARE_PATHS[@]+"${HOST_FIRMWARE_PATHS[@]}"}"; do
        [[ -d "$fw_path" ]] && extra_args+=("-b" "${fw_path}:${fw_path}")
    done

    local fw_cmds=""
    for fw_path in "${HOST_FIRMWARE_PATHS[@]+"${HOST_FIRMWARE_PATHS[@]}"}"; do
        fw_cmds+="cp -a '${fw_path}' /build/gen/root/lib/firmware/ 2>/dev/null || true; "
    done

    run_with_build_and_source "$sandbox_dir" "$build_dir" "$source_dir" \
      ${extra_args[@]+"${extra_args[@]}"} -- "
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

        # Build initramfs
        kver=\$(ls /build/gen/root/lib/modules/ | head -1)
        [[ -z \"\$kver\" ]] && { echo 'Error: no kernel modules found'; exit 1; }
        dracutbasedir=/usr/lib/dracut \
        DRACUT_INSTALL=/usr/lib/dracut/dracut-install \
          dracut -f --no-early-microcode --no-kernel \
            -m '${DRACUT_MODULES}' --gzip \
            --sysroot /build/gen/root \
            --tmpdir /tmp \
            /build/gen/boot/initramfs.img \"\$kver\"

        # U-Boot script + uInitrd
        mkimage -A riscv -T script -C none -d /scripts/boards/ky-x1/boot.cmd /build/gen/boot/boot.scr
        mkimage -A riscv -O linux -T ramdisk -C gzip -d /build/gen/boot/initramfs.img /build/gen/boot/uInitrd
    "
}
