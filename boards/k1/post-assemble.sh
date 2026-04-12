set -e

# Board firmware overlay
mkdir -p /build/gen/root/lib/firmware
cp -a /build/firmware/${BOARD_FIRMWARE_OVERLAY}/. /build/gen/root/lib/firmware/

# Host firmware (wifi, etc.)
for fw_path in ${HOST_FIRMWARE_PATHS[@]+"${HOST_FIRMWARE_PATHS[@]}"}; do
    cp -a "${fw_path}" /build/gen/root/lib/firmware/ 2>/dev/null || true
done

# Dracut firmware hint
mkdir -p /build/gen/root/etc/dracut.conf.d
echo 'install_items+=" /lib/firmware/esos.elf "' > /build/gen/root/etc/dracut.conf.d/firmware.conf

# U-Boot environment
printf 'console=%s\ninit=/init\nbootdelay=0\nloglevel=%s\nknl_name=%s\nramdisk_name=%s\nset_root_arg=setenv bootargs root=%s\n' \
    "${BOOT_CONSOLE}" "${BOOT_LOGLEVEL}" "${BOOT_KERNEL_NAME}" "${BOOT_RAMDISK_NAME}" "${BOOT_ROOT_DEV}" \
    > /build/gen/boot/env_${BOARD_NAME}-x.txt
