#!/bin/bash
set -e

kver=$(ls /build/gen/root/lib/modules/ 2>/dev/null | head -1)
[ -z "$kver" ] && { echo 'Error: no kernel modules found'; exit 1; }

mkdir -p "/build/gen/boot/zhihe/${kver}"
cp /build/gen/boot/*.dtb "/build/gen/boot/zhihe/${kver}/" 2>/dev/null || true

mkdir -p /build/gen/boot/extlinux
cat > /build/gen/boot/extlinux/extlinux.conf <<EXTEOF
DEFAULT a210
TIMEOUT 30

LABEL a210
    MENU LABEL Zhihe A210 (rv64gcv VLEN=128)
    LINUX /${BOOT_KERNEL_NAME:-Image}
    FDT /zhihe/${kver}/${BOOT_DTB_NAME:-a210-dev.dtb}
    APPEND console=ttyS4,115200 root=${BOOT_ROOT_DEV} rw rootwait earlycon loglevel=4
EXTEOF

cp /build/linux/arch/riscv/boot/Image "/build/gen/boot/${BOOT_KERNEL_NAME:-Image}"

ITB_STAGE=$(mktemp -d)
cp "/build/gen/boot/zhihe/${kver}/${BOOT_DTB_NAME:-a210-dev.dtb}" "${ITB_STAGE}/${BOOT_DTB_NAME:-a210-dev.dtb}"
cp /build/opensbi/build/platform/generic/firmware/fw_dynamic.bin "${ITB_STAGE}/fw_dynamic.bin"
cp /build/u-boot/u-boot.bin "${ITB_STAGE}/u-boot.bin"
gzip -fkn9 "${ITB_STAGE}/fw_dynamic.bin"
gzip -fkn9 "${ITB_STAGE}/u-boot.bin"

cat > "${ITB_STAGE}/riscv-boot.its" <<ITSEOF
/dts-v1/;

/ {
    description = "Zhihe A210 Boot FIT";
    #address-cells = <1>;

    images {
        fdt-dev {
            description = "A210 DEV DT";
            data = /incbin/("${BOOT_DTB_NAME:-a210-dev.dtb}");
            type = "flat_dt";
            arch = "riscv";
            compression = "none";
            load = <0x8c000000>;
            hash { algo = "sha256"; };
        };
        opensbi-1 {
            description = "OpenSBI fw_dynamic";
            data = /incbin/("fw_dynamic.bin.gz");
            type = "firmware";
            os = "opensbi";
            arch = "riscv";
            compression = "gzip";
            load = <0x80000000>;
            entry = <0x80000000>;
            hash { algo = "sha256"; };
        };
        u-boot-1 {
            description = "U-Boot";
            data = /incbin/("u-boot.bin.gz");
            type = "firmware";
            os = "u-boot";
            arch = "riscv";
            compression = "gzip";
            load = <0x90000000>;
            entry = <0x90000000>;
            hash { algo = "sha256"; };
        };
    };

    configurations {
        default = "a210-dev";
        a210-dev {
            description = "A210 DEV";
            fdt = "fdt-dev";
            firmware = "opensbi-1";
            loadables = "u-boot-1";
        };
    };
};
ITSEOF

mkdir -p /build/u-boot
( cd "${ITB_STAGE}" && mkimage -f riscv-boot.its /build/u-boot/riscv-boot.itb )

rvbl_wrap() {
    local bin="$1" payload="$2" out="$3"
    python3 - "$bin" "$payload" "$out" <<'PYEOF'
import os, struct, sys
bin_path, payload_path, out_path = sys.argv[1:4]
with open(bin_path, "rb") as f: bin_data = f.read()
pad = (-len(bin_data)) % 16
pad_bin_size = len(bin_data) + pad
header = b"\x6f\x00\x10\x00"
header += b"RVBL"
header += struct.pack("<I", pad_bin_size + 2048)
if payload_path and payload_path != "none" and os.path.exists(payload_path):
    with open(payload_path, "rb") as f: payload = f.read()
    header += struct.pack("<I", len(payload))
else:
    payload = b""
    header += struct.pack("<I", 0)
header += b"\x00" * 2032
with open(out_path, "wb") as f:
    f.write(header)
    f.write(bin_data)
    f.write(b"\x00" * pad)
    if payload: f.write(payload)
PYEOF
}

SPL_BIN=/build/u-boot/spl/u-boot-spl.bin
[ -f "$SPL_BIN" ] || { echo "ERROR: SPL not built at $SPL_BIN" >&2; exit 1; }

if [ -f /build/u-boot/zhihe-rvbl.bin ]; then
    magic=$(head -c 4 /build/u-boot/zhihe-rvbl.bin)
    [ "$magic" = "RVBL" ] || echo "WARN: zhihe-rvbl.bin missing RVBL magic (got: $magic)" >&2
fi

rvbl_wrap "$SPL_BIN" /build/u-boot/riscv-boot.itb /build/u-boot/spl-with-fit-rvbl.bin
rvbl_wrap "$SPL_BIN" none /build/u-boot/u-boot-spl-rvbl.bin

BLOB_DIR=/scripts/boards/zhihe-a210/firmware
if [ -f "${BLOB_DIR}/bootzero-rvbl.bin" ]; then
    cp "${BLOB_DIR}/bootzero-rvbl.bin" /build/u-boot/bootzero-rvbl.bin
fi
if [ -f "${BLOB_DIR}/bootzero2.bin" ]; then
    cp "${BLOB_DIR}/bootzero2.bin" /build/u-boot/bootzero2.bin
    OUT=/build/u-boot/emmc_boot-loader.img
    cat /build/u-boot/bootzero2.bin /build/u-boot/u-boot-spl-rvbl.bin > "$OUT"
    truncate -s 851968 "$OUT"
    cat /build/u-boot/riscv-boot.itb >> "$OUT"
    echo "[*] generated $OUT ($(stat -c %s $OUT) bytes)"
fi

python3 - <<'PYEOF'
import struct, binascii
entries = [
    "arch=riscv",
    "baudrate=115200",
    "board=a210-evb",
    "bootdelay=3",
    "kernel_addr=0x80200000",
    "dtb_addr=0x8c000000",
    "bootargs=console=ttyS4,115200 root=PARTLABEL=rootfs rw rootwait earlycon clk_ignore_unused loglevel=4",
    "bootcmd=ext4load mmc 0:3 ${kernel_addr} Image && ext4load mmc 0:3 ${dtb_addr} a210-dev.dtb && booti ${kernel_addr} - ${dtb_addr}",
]
data = b''
for e in entries:
    data += e.encode('ascii') + b'\x00'
data += b'\x00'
ENV_SIZE = 16384
PAYLOAD = ENV_SIZE - 5
padded = data + b'\xff' * (PAYLOAD - len(data))
crc = binascii.crc32(padded) & 0xffffffff
copy = struct.pack('<I', crc) + b'\x01' + padded
open('/build/u-boot/uboot_env.img', 'wb').write(copy + copy)
print(f"[*] generated /build/u-boot/uboot_env.img (32768 bytes, redundant 16K, CRC={crc:08x})")
PYEOF

# Stage everything flash.sh needs at the /build top level:
# `image export --all` copies only top-level files (no subdirectories).
cp -f /scripts/boards/zhihe-a210/flash.sh /build/flash.sh
for f in spl-with-fit-rvbl.bin bootzero-rvbl.bin emmc_boot-loader.img uboot_env.img; do
    [ -f "/build/u-boot/$f" ] && cp -f "/build/u-boot/$f" "/build/$f"
done

rm -rf "${ITB_STAGE}"
