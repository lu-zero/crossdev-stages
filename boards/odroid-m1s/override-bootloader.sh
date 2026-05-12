set -e

# 1. Build ARM Trusted Firmware (BL31)
# RK3566 shares the rk3568 TFA platform
make -C /build/tfa PLAT=${TFA_PLAT} CROSS_COMPILE=${CROSS_COMPILE} bl31 -j$(nproc)

# 2. Resolve DDR blob (glob pattern, exclude eyescan, pick latest)
DDR_BLOB=$(ls /build/rkbin/${RKBIN_DDR} 2>/dev/null | grep -v eyescan | sort -V | tail -1)
[ -z "$DDR_BLOB" ] && { echo "Error: no DDR blob matching ${RKBIN_DDR}" >&2; exit 1; }
echo "Using DDR blob: $DDR_BLOB"

# 3. Build U-Boot with TF-A and DDR blob
export ROCKCHIP_TPL="$DDR_BLOB"
export BL31=/build/tfa/build/${TFA_PLAT}/release/bl31/bl31.elf

make -C /build/u-boot CROSS_COMPILE=${CROSS_COMPILE} ${U_BOOT_DEFCONFIG}
make -C /build/u-boot CROSS_COMPILE=${CROSS_COMPILE} -j$(nproc)
