set -e

# SM1 uses g12a FIP flow; BL31 stays pre-built since TFA's bl31.bin needs
# conversion to Amlogic's bl31.img format (not yet wired up).
mkdir -p /build/u-boot-sd
cd /build/firmware
./build-fip.sh "${BOARD_NAME}" /build/u-boot/u-boot.bin /build/u-boot-sd/
