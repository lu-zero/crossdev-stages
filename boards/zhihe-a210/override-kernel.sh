#!/bin/bash
set -e

cd /build/linux

make ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" "${KERNEL_DEFCONFIG}"

BLOB_SRC=/scripts/boards/zhihe-a210/firmware/a210-aon.bin
if [ -f "${BLOB_SRC}" ]; then
    echo "[*] baking a210-aon.bin"
    install -D "${BLOB_SRC}" /build/linux/firmware/a210-aon.bin
    scripts/config \
        --set-str EXTRA_FIRMWARE "a210-aon.bin" \
        --set-str EXTRA_FIRMWARE_DIR "firmware"
else
    echo "[!] a210-aon.bin not found at ${BLOB_SRC}"
fi

make ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" olddefconfig
make ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" -j"$(nproc)"
