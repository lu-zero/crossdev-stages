#!/bin/bash
# override-assemble.sh for VSRVES01.
#
# Architecture: phram-rootfs.  Vendor VSOS loads four payloads to DDR
# via the DDRLoad command from startup.txt:
#
#   rvlbne.bin    0x80000000  RISC-V bootloader (vendor)
#   _ddrBlob      ~0x80007000 vendor RvParam + clock + memory config
#   catboard.dtb  ~0x80007fe8 device tree
#   image.sqfs    0x83000000  our squashfs rootfs (80 MiB region)
#   linux61.vri   0x80400000  the kernel (VRI internal addressing)
#
# The sqfs region is protected from the Linux allocator by a
# /reserved-memory node written into the DTB below (memmap= is x86-only;
# the riscv kernel does not parse it).  Kernel cmdline (/chosen/bootargs):
#   phram.phram=rootfs,... exposes the region as MTD device /dev/mtd0;
#   root=/dev/mtdblock0 + rootfstype=squashfs mounts it directly — no
#   cpio, no initramfs, kernel goes straight to /sbin/init from the
#   squashfs (sysvinit on this image).
#
# After this hook, default `pack` step calls genimage to wrap
# /build/gen/boot/ into the superfloppy FAT image.
set -euo pipefail

GEN=/build/gen
KSRC=/build/linux

# Wipe any stale Image.vri left by a prior run — only linux61.vri is what
# startup.txt references.
rm -f "${GEN}/boot/Image.vri"
install -Dm644 "${KSRC}/arch/riscv/boot/Image.vri" "${GEN}/boot/linux61.vri"

# Vendor DDRLoad copies this verbatim to 0x83000000 (80 MiB window up to
# the 128 MiB DDR top at 0x88000000) — kernel sees it through
# CONFIG_MTD_PHRAM and mounts it as /dev/mtdblock0.
install -Dm644 "${KSRC}/image.sqfs" "${GEN}/boot/image.sqfs"

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
# would otherwise bloat the FAT.  FAT lookups are case-insensitive so
# mixed casing (`CatsEyes` vs `CatSEyes.dr3`) works.
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

# Vendor startup.txt with two edits:
#   1. uncomment `term -a` so VSDSP forwards its UART to the RV side
#      after ddrload — Linux console visibility on the same serial cable.
#   2. extend the ddrload line to load image.sqfs at 0x83000000 (phram
#      rootfs payload, 80 MiB window).  Inserts `-a0x83000000
#      -bimage.sqfs` right before the trailing `linux61.vri` (the VRI
#      file MUST come last so ddrload's "current address" doesn't
#      shift mid-kernel-load).
install -m 0644 /build/firmware/vsrv_root/startup.txt "${GEN}/boot/startup.txt"
sed -i 's/^#[Tt]erm -a/Term -a/' "${GEN}/boot/startup.txt"
sed -i 's|\(ddrload [^[:cntrl:]]*\)\<linux61.vri\>|\1-a0x83000000 -bimage.sqfs linux61.vri|' \
    "${GEN}/boot/startup.txt"

# Stage the in-tree DTB.  Filename MUST be `catboard.dtb` (8.3) — vendor
# startup.txt invokes `ddrload ... -bcatboard.dtb ...`.  Override
# chosen.bootargs for the phram-rootfs path:
#
#   phram.phram=rootfs,...  expose the region as MTD (/dev/mtd0 → /dev/mtdblock0)
#   root=/dev/mtdblock0     mount the phram-backed device as rootfs
#   rootfstype=squashfs ro  squashfs, read-only (it IS read-only anyway)
#
# `phram.phram=` (not `phram=`): the driver is built in, so the module
# parameter needs the <modname>.<param> prefix.  Console order matters —
# the LAST console= becomes /dev/console; ttyUL0 is the user's cable.
install -Dm644 "${KSRC}/arch/riscv/boot/dts/vlsi/vsrves01-catboard.dtb" \
               "${GEN}/boot/catboard.dtb"
fdtput -c "${GEN}/boot/catboard.dtb" /chosen
fdtput -ts "${GEN}/boot/catboard.dtb" /chosen bootargs \
    'console=ttyS0,115200 console=ttyUL0,1000000 phram.phram=rootfs,0x83000000,0x5000000 root=/dev/mtdblock0 rootfstype=squashfs ro rootwait'

# Reserve the phram window (0x83000000 + 0x5000000 = 80 MiB up to the
# DDR top at 0x88000000) from the Linux page allocator via a
# /reserved-memory node.  The previous approach used memmap=nn$ss on the
# command line, but that parameter is x86/MIPS/Xtensa-only — the riscv
# kernel silently ignores it, and the DTS /memory node (64 MiB ending at
# 0x84000000) overlaps the window by 16 MiB, letting memblock's top-down
# allocations stomp the head of the squashfs.  no-map keeps the region
# out of the linear map entirely; phram accesses it via its own mapping.
# Cells match the DTS root (#address-cells = #size-cells = <1>).
fdtput -c  "${GEN}/boot/catboard.dtb" /reserved-memory
fdtput -tx "${GEN}/boot/catboard.dtb" /reserved-memory '#address-cells' 1
fdtput -tx "${GEN}/boot/catboard.dtb" /reserved-memory '#size-cells' 1
fdtput -tx "${GEN}/boot/catboard.dtb" /reserved-memory ranges
fdtput -c  "${GEN}/boot/catboard.dtb" /reserved-memory/phram@83000000
fdtput -tx "${GEN}/boot/catboard.dtb" /reserved-memory/phram@83000000 reg 0x83000000 0x5000000
fdtput -tx "${GEN}/boot/catboard.dtb" /reserved-memory/phram@83000000 no-map

echo "::: post-assemble: boot dir contents:"
du -sh "${GEN}/boot"/* 2>/dev/null | sort -h
echo "::: post-assemble: done"
