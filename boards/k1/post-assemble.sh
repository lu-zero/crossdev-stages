set -e

# U-Boot environment (kernel command line wired up here).
printf 'console=%s\ninit=/init\nbootdelay=0\nloglevel=%s\nknl_name=%s\nramdisk_name=%s\nset_root_arg=setenv bootargs root=%s\n' \
    "${BOOT_CONSOLE}" "${BOOT_LOGLEVEL}" "${BOOT_KERNEL_NAME}" "${BOOT_RAMDISK_NAME}" "${BOOT_ROOT_DEV}" \
    > /build/gen/boot/env_${BOARD_NAME}-x.txt
