#!/bin/bash
set -e

# Zhihe A210 post-assemble:
#   1. Build riscv-boot.itb FIT (kernel/initrd/dtb + fw_dynamic + u-boot)
#   2. Wrap u-boot SPL with -rvbl header → spl-with-fit-rvbl.bin
#   3. Wrap bootzero (vendor blob, if present) → bootzero-rvbl.bin
#   4. Stage a210-dev.dtb under vendor-expected /boot path
#   5. Write extlinux.conf for u-boot to boot the kernel
#
# Vendor reference: zhihe-a210-u-boot/board/zhihe/common/script/
#                       {generate_firmware.sh, generate_itb.sh}
# RVBL header layout (16-byte aligned):
#   magic "RVBL" + LE u32 (pad_bin_size + 2048) + LE u32 payload_size
#   + 2032 zero bytes + payload (+ pad to 16) + optional payload
# All produced by `generate_firmware.sh rvbl <bin> <payload> <out>`.

kver=$(ls /build/gen/root/lib/modules/ 2>/dev/null | head -1)
[ -z "$kver" ] && { echo 'Error: no kernel modules found'; exit 1; }

# Place DTB under vendor-expected path: /boot/zhihe/<kver>/
mkdir -p "/build/gen/boot/zhihe/${kver}"
mv /build/gen/boot/*.dtb "/build/gen/boot/zhihe/${kver}/" 2>/dev/null || true

# Extlinux config — vendor u-boot proper reads /extlinux/extlinux.conf from
# the boot ext4 partition.  No INITRD line: the OSL kernel statically pulls
# in eMMC / Ethernet / pinctrl / PMIC, mount-root-via-PARTUUID works directly
# (PROVISION_RUNBOOK.md confirms this is the bench-validated path).
#
# APPEND matches the production bootargs documented in PROVISION_RUNBOOK.md:
#   console=ttyS4,115200 root=PARTUUID=…-0004 rw rootwait earlycon loglevel=4
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

# Stage kernel Image alongside extlinux (vendor u-boot SPL FS path).
cp /build/linux/arch/riscv/boot/Image "/build/gen/boot/${BOOT_KERNEL_NAME:-Image}"

# ── Build riscv-boot.itb FIT ───────────────────────────────────────────
# FIT contains:
#   - a210-dev.dtb     (load 0x8c000000)
#   - fw_dynamic.bin   (firmware, opensbi, gzip, load+entry 0x80000000)
#   - u-boot.bin       (firmware, u-boot,  gzip, load+entry 0x90000000)
# u-boot SPL reads this FIT via CONFIG_SPL_LOAD_FIT_FULL=y.
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

# ── Wrap SPL with RVBL header ──────────────────────────────────────────
# generate_rvbl: magic "RVBL" + LE32(pad_bin_size+2048) + LE32(payload_size)
# + 2032 zero bytes + bin (+ pad to 16) + payload
rvbl_wrap() {
    local bin="$1" payload="$2" out="$3"
    python3 - "$bin" "$payload" "$out" <<'PYEOF'
import os, struct, sys
bin_path, payload_path, out_path = sys.argv[1:4]
with open(bin_path, "rb") as f: bin_data = f.read()
pad = (-len(bin_data)) % 16
pad_bin_size = len(bin_data) + pad
header = b"RVBL"
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

# Vendor SPL build target is `zhihe-rvbl.bin` (CONFIG_BUILD_TARGET).
# But the raw SPL we need is u-boot-spl.bin; we add our own RVBL header.
SPL_BIN=/build/u-boot/spl/u-boot-spl.bin
[ -f "$SPL_BIN" ] || { echo "ERROR: SPL not built at $SPL_BIN" >&2; exit 1; }

# Magic check: vendor's `zhihe-rvbl.bin` (if produced by u-boot build)
# should start with "RVBL".  Validate when present.
if [ -f /build/u-boot/zhihe-rvbl.bin ]; then
    magic=$(head -c 4 /build/u-boot/zhihe-rvbl.bin)
    [ "$magic" = "RVBL" ] || echo "WARN: zhihe-rvbl.bin missing RVBL magic (got: $magic)" >&2
fi

# spl-with-fit-rvbl.bin: RVBL-wrap SPL with riscv-boot.itb as payload
rvbl_wrap "$SPL_BIN" /build/u-boot/riscv-boot.itb /build/u-boot/spl-with-fit-rvbl.bin
# u-boot-spl-rvbl.bin: RVBL-wrap SPL alone (used inside btz chain)
rvbl_wrap "$SPL_BIN" none /build/u-boot/u-boot-spl-rvbl.bin

# Vendor closed blob (bootzero-rvbl.bin) is required for the first
# fastboot recovery step on bare boards.  See firmware/README.md.
# Stage it if user dropped it under firmware/.
BLOB_DIR=/scripts/boards/zhihe-a210/firmware
if [ -f "${BLOB_DIR}/bootzero-rvbl.bin" ]; then
    cp "${BLOB_DIR}/bootzero-rvbl.bin" /build/u-boot/bootzero-rvbl.bin
fi
if [ -f "${BLOB_DIR}/bootzero2.bin" ]; then
    cp "${BLOB_DIR}/bootzero2.bin" /build/u-boot/bootzero2.bin
fi

rm -rf "${ITB_STAGE}"
