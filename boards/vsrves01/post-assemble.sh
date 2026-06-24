#!/bin/bash
# post-assemble.sh for VSRVES01.
#
# Runs AFTER the cross-stages driver has populated:
#   /build/gen/root/         - cross-emerged @system rootfs
#   /build/gen/boot/         - kernel modules, dtb, etc.
# Already-built artefacts:
#   /build/linux/arch/riscv/boot/Image.vri        - kernel with embedded
#                                                   stage-1 initramfs
#   /build/linux/arch/riscv/boot/dts/vlsi/...dtb  - in-tree dtb
#
# Responsibilities here:
#   1. Stage vendor VSOS boot blobs (rvlbne.bin, SYSR/*, startup.txt).
#   2. Stage kernel + dtb under /build/gen/boot/.
#   3. Apply the rootfs-overlay onto a staging copy of /build/gen/root and
#      bake it into a small ext4 image at /build/gen/boot/rootfs.ext4 (a
#      regular file inside the FAT superfloppy, attached at runtime via
#      /dev/loop0).
#
# rootfs.ext4 is built small (400 MiB).  First boot runs
# /etc/init.d/grow-rootfs which truncates the file to fill remaining FAT
# free space and online-resizes ext4.
set -euo pipefail

GEN=/build/gen
KSRC=/build/linux
SCRIPTS=/scripts/boards/vsrves01
ROOTFS_DIR="${GEN}/root"
STAGING="${GEN}/rootfs-staging"
OVERLAY="${SCRIPTS}/rootfs-overlay"
EXT4_OUT="${GEN}/boot/rootfs.ext4"
EXT4_SIZE_M=400

# --- Stage kernel + dtb ----------------------------------------------------
install -Dm644 "${KSRC}/arch/riscv/boot/Image.vri" "${GEN}/boot/linux61.vri"

install -Dm644 /build/firmware/vsrv_root/rvlbne.bin "${GEN}/boot/rvlbne.bin"

# VSOS root-level payloads referenced by startup.txt: rvparam.bin is the
# persistent config blob RvParam reads, shell.ap3 is the actual VSDSP shell
# binary loaded by !Shell, Welcome.txt is printed by `Type +v Welcome.txt`,
# README.TXT is a 1.3 KB quality-of-life help text.  linux61.vri is built
# above from the kernel Image (not copied).
install -m 0644 /build/firmware/vsrv_root/rvparam.bin   "${GEN}/boot/rvparam.bin"
install -m 0644 /build/firmware/vsrv_root/shell.ap3     "${GEN}/boot/shell.ap3"
install -m 0644 /build/firmware/vsrv_root/Welcome.txt   "${GEN}/boot/Welcome.txt"
install -m 0644 /build/firmware/vsrv_root/README.TXT    "${GEN}/boot/README.TXT"

# SYSR/ holds the driver/command .dr3 modules + preg.dat lookup table +
# kernel.sym symbol table.  startup.txt invokes Driver/SetClock/PReg/
# RvParam/ddrload/AuOutput/Type/!Shell which all fopen() SYSR/*.dr3.
# Copy only the modules actually used (verified against startup.txt);
# the vendor zip ships ~60 unused .dr3 (DecMP3, DecFlac, Edit, ...) that
# would otherwise bloat the FAT and cluster count.  FAT lookups are
# case-insensitive so mixed casing (`CatsEyes` vs `CatSEyes.dr3`) works.
install -d -m 0755 "${GEN}/boot/SYSR"
for f in BadBit.dr3 Driver.dr3 SetClock.dr3 CatSEyes.dr3 UartIn.dr3 \
         PReg.dr3 preg.dat RVParam.dr3 ParamSpl.dr3 Trace.dr3 \
         DDRLoad.dr3 auieth.dr3 auodac.dr3 AuOutput.dr3 AuxPlayB.dr3 \
         Type.dr3 Shell.dr3 GetCmd.dr3 Dir.dr3 Cd.dr3 CopyF.dr3 Del.dr3 \
         More.dr3 DiskFree.dr3 Edit.dr3 Edit.ini Time.dr3 Tasks.dr3 \
         Term.dr3 RvTerm.dr3 kernel.sym; do
    install -m 0644 "/build/firmware/vsrv_root/SYSR/$f" \
                    "${GEN}/boot/SYSR/$f"
done

# Vendor startup.txt verbatim, but uncomment `term -a` so VSDSP forwards
# its UART to the RV side after ddrload - gives the user Linux console
# visibility on the same physical serial cable they already use for VSOS.
install -m 0644 /build/firmware/vsrv_root/startup.txt "${GEN}/boot/startup.txt"
sed -i 's/^#Term -a/Term -a/' "${GEN}/boot/startup.txt"

# Stage the in-tree DTB next to linux61.vri.  Override chosen.bootargs so
# the kernel finds the loop-mounted rootfs (mounted by stage-1 init); add
# console=ttyS0,115200 as a secondary console for convenience.
install -Dm644 "${KSRC}/arch/riscv/boot/dts/vlsi/vsrves01-catboard.dtb" \
               "${GEN}/boot/vsrves01-catboard.dtb"
fdtput -c "${GEN}/boot/vsrves01-catboard.dtb" /chosen
fdtput -ts "${GEN}/boot/vsrves01-catboard.dtb" /chosen bootargs \
    "console=ttyS0,115200 console=ttyUL0,1000000 memtest=0 rdinit=/init"

# --- Build stage-2 rootfs.ext4 --------------------------------------------
echo "::: post-assemble: preparing rootfs staging at ${STAGING}"
rm -rf "${STAGING}"
mkdir -p "${STAGING}"

# --reflink=auto for speed on btrfs/xfs; harmless on ext4.
cp -a --reflink=auto "${ROOTFS_DIR}/." "${STAGING}/"

echo "::: post-assemble: applying overlay from ${OVERLAY}"
cp -a "${OVERLAY}/." "${STAGING}/"

# Ensure runtime mount points exist.
mkdir -p "${STAGING}/proc" "${STAGING}/sys" "${STAGING}/dev" \
         "${STAGING}/run"  "${STAGING}/tmp" "${STAGING}/boot" \
         "${STAGING}/var/lib"
chmod 1777 "${STAGING}/tmp"

# /etc/shadow must not be world-readable.
[ -f "${STAGING}/etc/shadow" ] && chmod 0640 "${STAGING}/etc/shadow"

# Make sure the rc symlink is in place (overlay ships an empty default/
# dir; cp -a preserves that but we must add the symlink ourselves).
mkdir -p "${STAGING}/etc/runlevels/default"
if [ ! -L "${STAGING}/etc/runlevels/default/grow-rootfs" ]; then
    ln -sf /etc/init.d/grow-rootfs \
        "${STAGING}/etc/runlevels/default/grow-rootfs"
fi
chmod 0755 "${STAGING}/etc/init.d/grow-rootfs"

echo "::: post-assemble: building ${EXT4_OUT} (${EXT4_SIZE_M} MiB)"
mkdir -p "${GEN}/boot"
rm -f "${EXT4_OUT}"

# mkfs.ext4 -d populates the image from a directory in one shot - no loop
# mount needed, no root needed.
mkfs.ext4 \
    -d "${STAGING}" \
    -L vsrves01-root \
    -m 1 \
    -O ^has_journal,^huge_file,^metadata_csum_seed \
    -F \
    "${EXT4_OUT}" \
    "${EXT4_SIZE_M}M"

echo "::: post-assemble: rootfs.ext4 = $(stat -c %s "${EXT4_OUT}") bytes"
echo "::: post-assemble: done"
