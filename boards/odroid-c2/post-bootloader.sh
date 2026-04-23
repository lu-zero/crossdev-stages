set -e

# Amlogic FIP assembly. amlogic-boot-fip ships vendor BL2/BL30/BL301 (signed
# closed-source blobs from HardKernel) plus a matching pre-built bl31.bin.
#
# If board.conf sets TFA_REPO, we build our own BL31 from mainline TF-A and
# substitute it in. Leaving TFA_REPO unset falls back to the shipped bl31.bin.
if [ -n "${TFA_REPO:-}" ] && [ -n "${TFA_PLAT:-}" ]; then
    cp /build/tfa/build/"${TFA_PLAT}"/release/bl31.bin \
       /build/firmware/"${BOARD_NAME}"/bl31.bin
fi

# Output dir separate from u-boot/: the fip Makefile writes a ${O}/u-boot.bin
# (encrypted) which would otherwise clobber the mainline u-boot.bin input and
# confuse re-runs.
mkdir -p /build/u-boot-sd
cd /build/firmware
./build-fip.sh "${BOARD_NAME}" /build/u-boot/u-boot.bin /build/u-boot-sd/
