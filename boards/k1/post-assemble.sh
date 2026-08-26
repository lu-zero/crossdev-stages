set -e

# bianbu u-boot reads env_k1-x.txt and substitutes its variables into a
# vendor-defined `commonargs` template.  earlycon is one of those
# variables; leaving it unset produces a malformed bootargs.  Set it
# explicitly to `sbi` (matches Armbian's K1 cmdline) for usable serial
# output during early kernel init.
printf 'console=%s\ninit=/init\nbootdelay=0\nloglevel=%s\nearlycon=sbi\nknl_name=%s\nramdisk_name=%s\nset_root_arg=setenv bootargs root=%s\n' \
    "${BOOT_CONSOLE}" "${BOOT_LOGLEVEL}" "${BOOT_KERNEL_NAME}" "${BOOT_RAMDISK_NAME}" "${BOOT_ROOT_DEV}" \
    > /build/gen/boot/env_${BOARD_NAME}-x.txt
