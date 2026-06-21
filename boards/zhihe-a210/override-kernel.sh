#!/bin/bash
set -e

# Zhihe A210 kernel build:
#
# The board's AON (Always-On) subsystem runs on an E902 core that needs
# the closed `a210-aon.bin` firmware (~52K) for PMIC, RTC, reboot, and
# regulator control.  Without it the kernel boots but reboot/poweroff
# hang and some regulators stay off.
#
# Two ways to supply it:
#   (a) at runtime under /lib/firmware/zhihe/a210-aon.bin
#   (b) baked into the kernel via CONFIG_EXTRA_FIRMWARE
#
# We do (b) when the blob is present (mirrors how vendor buildroot does
# it and how K230/K3 handle similar AON cases) so the firmware is in the
# initramfs path from the very first boot.  Falls back to runtime if the
# blob is missing — see firmware/README.md for how to obtain it.

cd /build/linux

make ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" "${KERNEL_DEFCONFIG}"

BLOB_SRC=/scripts/boards/zhihe-a210/firmware/a210-aon.bin
if [ -f "${BLOB_SRC}" ]; then
    echo "[*] baking a210-aon.bin into kernel (CONFIG_EXTRA_FIRMWARE)"
    install -D "${BLOB_SRC}" /build/linux/firmware/zhihe/a210-aon.bin
    scripts/config \
        --set-str EXTRA_FIRMWARE "zhihe/a210-aon.bin" \
        --set-str EXTRA_FIRMWARE_DIR "firmware"
else
    echo "[!] a210-aon.bin not found — kernel will look for it at runtime"
    echo "    under /lib/firmware/zhihe/a210-aon.bin (see firmware/README.md)"
fi

make ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" olddefconfig
make ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" -j"$(nproc)"
