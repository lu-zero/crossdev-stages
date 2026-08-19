#!/bin/bash
set -e

# arm64 defconfig already covers RK3588 (panthor, VOP2, dw-hdmi-qp, hantro
# AV1, SCMI cpufreq, PMIC).  Three things it does not enable: the NPU
# (rocket, drivers/accel), the RK3588 video decoder (vdpu381), and the IOMMU
# the AV1 decoder sits behind.  av1d in rk3588-base.dtsi points at av1d_mmu,
# whose "verisilicon,iommu-1.2" is driven by VSI_IOMMU, which nothing selects.
# It has to be builtin: as a module the decoder can bind first, hit the 10s
# deferred-probe timeout and stay bound with no IOMMU, silently.
cd /build/linux
make ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" "${KERNEL_DEFCONFIG}"
scripts/config --enable DRM_ACCEL \
               --module DRM_ACCEL_ROCKET \
               --module VIDEO_ROCKCHIP_VDEC \
               --enable VSI_IOMMU
make ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" olddefconfig

# olddefconfig silently drops symbols whose dependencies are unmet; fail here
# rather than shipping an image without the NPU.
for sym in CONFIG_DRM_ACCEL_ROCKET CONFIG_DRM_PANTHOR CONFIG_VIDEO_ROCKCHIP_VDEC \
           CONFIG_VSI_IOMMU; do
    grep -q "^${sym}=[ym]$" .config || { echo "Error: ${sym} not enabled"; exit 1; }
done

make ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" WERROR=0 -j"$(nproc)"
