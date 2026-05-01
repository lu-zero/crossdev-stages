set -e

mkdir -p /build/gen/root/usr/lib/firmware
cp -a /build/firmware/${BOARD_FIRMWARE_OVERLAY}/. /build/gen/root/usr/lib/firmware/

for fw_path in ${HOST_FIRMWARE_PATHS[@]+"${HOST_FIRMWARE_PATHS[@]}"}; do
    cp -a "${fw_path}" /build/gen/root/usr/lib/firmware/ 2>/dev/null || true
done
