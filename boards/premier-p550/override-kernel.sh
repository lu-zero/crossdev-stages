#!/bin/bash
set -e

# v7.2-rc1 defconfig enables ARCH_ESWIN; the EIC7700 drivers still
# need turning on.

make -C /build/linux ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" "${KERNEL_DEFCONFIG}"

cat >> /build/linux/.config <<'EOF'
CONFIG_ARCH_ESWIN=y
CONFIG_PINCTRL_EIC7700=y
CONFIG_DWMAC_EIC7700=y
CONFIG_RESET_EIC7700=y
CONFIG_COMMON_CLK_ESWIN=y
CONFIG_COMMON_CLK_EIC7700=y
CONFIG_PHY_EIC7700_SATA=y

# P550 is RVA20U64 base (rv64imafdc + zba + zbb): no Vector, no
# T-Head/Andes/MIPS vendor extensions.  Keep the boot-time alternatives
# patcher from rewriting for extensions this core lacks.
# CONFIG_RISCV_ISA_V is not set
# CONFIG_RISCV_ISA_V_DEFAULT_ENABLE is not set
# CONFIG_RISCV_ISA_XTHEADVECTOR is not set
# CONFIG_RISCV_ISA_VENDOR_EXT_THEAD is not set
# CONFIG_RISCV_ISA_VENDOR_EXT_ANDES is not set
# CONFIG_RISCV_ISA_VENDOR_EXT_MIPS is not set
# CONFIG_RISCV_ALTERNATIVE_EARLY is not set
EOF

make -C /build/linux ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" olddefconfig
make -C /build/linux ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" WERROR=0 -j"$(nproc)"
