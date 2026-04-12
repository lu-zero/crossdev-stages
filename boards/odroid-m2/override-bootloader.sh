set -e

# 1. Build ARM Trusted Firmware (BL31)
make -C /build/tfa PLAT=${TFA_PLAT} CROSS_COMPILE=${CROSS_COMPILE} bl31 -j$(nproc)

# 2. Build U-Boot with TF-A and DDR blob
export ROCKCHIP_TPL=/build/rkbin/${RKBIN_DDR}
export BL31=/build/tfa/build/${TFA_PLAT}/release/bl31/bl31.elf

make -C /build/u-boot CROSS_COMPILE=${CROSS_COMPILE} ${U_BOOT_DEFCONFIG}
make -C /build/u-boot CROSS_COMPILE=${CROSS_COMPILE} -j$(nproc)
