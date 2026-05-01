set -e

# Stage firmware into the rootfs BEFORE default_assemble runs dracut, so
# `INITRAMFS_INSTALL` files are present when dracut bakes the initramfs.
# Use /usr/lib/firmware (canonical location); /lib will be set up as a
# symlink to usr/lib by `cp -a /target` in default_assemble.

mkdir -p /build/gen/root/usr/lib/firmware
cp -a /build/firmware/${BOARD_FIRMWARE_OVERLAY}/. /build/gen/root/usr/lib/firmware/

for fw_path in ${HOST_FIRMWARE_PATHS[@]+"${HOST_FIRMWARE_PATHS[@]}"}; do
    cp -a "${fw_path}" /build/gen/root/usr/lib/firmware/ 2>/dev/null || true
done
