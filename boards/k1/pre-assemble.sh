set -e

# Stage firmware into the rootfs BEFORE default_assemble runs dracut, so
# `INITRAMFS_INSTALL` files are present when dracut bakes the initramfs.

mkdir -p /build/gen/root /build/gen/root/lib/firmware
cp -a /build/firmware/${BOARD_FIRMWARE_OVERLAY}/. /build/gen/root/lib/firmware/

for fw_path in ${HOST_FIRMWARE_PATHS[@]+"${HOST_FIRMWARE_PATHS[@]}"}; do
    cp -a "${fw_path}" /build/gen/root/lib/firmware/ 2>/dev/null || true
done
