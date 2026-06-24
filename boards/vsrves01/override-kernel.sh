#!/bin/bash
# override-kernel.sh for VSRVES01.
#
# Two-stage kernel build:
#
#   Stage 1: apply patches, build kernel with a placeholder empty
#            catboard.cpio so all Make targets succeed.
#
#   Stage 2: now that /build/gen/root has been populated by the cross-emerge
#            (busybox-static available), assemble a real stage-1 initramfs
#            from initramfs/initramfs.list (busybox + /init + dev nodes) and
#            rebuild Image.vri with that cpio embedded.
#
# NOTE: assemble runs AFTER kernel in BUILD_STEPS, which would normally mean
# rootfs isn't available here yet.  But /build/gen/root is populated at the
# start of `default_assemble` BEFORE the post-assemble hook fires; the place
# the static busybox is needed (as initramfs payload) is INSIDE Image.vri,
# so we must do both passes from this hook.  The driver runs this script
# once during the kernel step; the rootfs has been cross-emerged in the
# preceding deps/checkout phases and lives under /build/sysroot or similar.
# Adjust ROOTFS_SRC below if the layout differs.
#
# vsrves01 upstream-bound patch set (11 patches under patches/) carries:
#   - DT bindings + vsrves01-catboard.dts
#   - vlsi-mac in-tree Ethernet driver
#   - clocksource min_delta fix
#   - SBI base-extension stubs (firmware lacks them)
#   - bin2vri host tool + Image.vri make target
#   - vsrves01_defconfig
set -euo pipefail

KSRC=/build/linux
SCRIPTS=/scripts/boards/vsrves01
# Cross-emerged @system root populated by the musl rv32 toolchain workflow.
# Adjusted to whichever path the rv32-musl crossdev pipeline lands at.
ROOTFS_DIR="${ROOTFS_DIR:-/build/gen/root}"

INITRAMFS_DIR="${SCRIPTS}/initramfs"
GEN_INIT_CPIO="${KSRC}/usr/gen_init_cpio"
LIST_SRC="${INITRAMFS_DIR}/initramfs.list"
LIST_RESOLVED="${KSRC}/initramfs.list.resolved"
BUSYBOX_DST="${KSRC}/usr/initramfs-busybox"
CPIO_DST="${KSRC}/catboard.cpio"

# --- Stage 0: apply patches ------------------------------------------------
echo "::: override-kernel: applying patches"
cd "${KSRC}"
for p in "${SCRIPTS}/patches/"*.patch; do
    git apply --whitespace=nowarn "$p"
done

# --- Stage 1: initial kernel build with placeholder cpio -------------------
# vsrves01_defconfig has CONFIG_INITRAMFS_SOURCE="catboard.cpio" with all
# rd_* decompressors disabled - the file must exist as a raw cpio next to
# the kernel build root.  Start with an empty placeholder so make succeeds.
echo "::: override-kernel: stage 1 (placeholder cpio)"
: > "${CPIO_DST}"

make ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" "${KERNEL_DEFCONFIG}"
make ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" WERROR=0 -j"$(nproc)" \
    Image.vri vlsi/vsrves01-catboard.dtb modules

# --- Stage 2: stage busybox + /init, generate real cpio, rebuild Image -----
echo "::: override-kernel: stage 2 (real initramfs)"

# Pick the static busybox out of the cross-emerged rootfs.
if [ -x "${ROOTFS_DIR}/bin/busybox" ]; then
    BUSYBOX_SRC="${ROOTFS_DIR}/bin/busybox"
elif [ -x "${ROOTFS_DIR}/bin/busybox.static" ]; then
    BUSYBOX_SRC="${ROOTFS_DIR}/bin/busybox.static"
else
    echo "!! cannot find busybox in ${ROOTFS_DIR}/bin/ - skip stage 2" >&2
    echo "!! (rebuild after workflow A finishes to embed real initramfs)" >&2
    exit 0
fi

# Confirm it's actually statically linked - dynamic busybox would die in
# initramfs because there is no /lib/ld-musl-* present.
if file "${BUSYBOX_SRC}" | grep -q 'dynamically linked'; then
    echo "!! ${BUSYBOX_SRC} is dynamically linked; need busybox-static" >&2
    exit 1
fi

install -m 0755 "${BUSYBOX_SRC}" "${BUSYBOX_DST}"
echo "::: override-kernel: staged $(stat -c %s "${BUSYBOX_DST}") byte busybox"

# gen_init_cpio is built as a side-effect of any usr/ target in stage 1, but
# rebuild on demand if it somehow isn't there.
if [ ! -x "${GEN_INIT_CPIO}" ]; then
    make -C "${KSRC}" usr/gen_init_cpio
fi

echo "::: override-kernel: resolving initramfs.list variables"
sed -e "s|\${KSRC}|${KSRC}|g" \
    -e "s|\${SCRIPTS}|${SCRIPTS}|g" \
    "${LIST_SRC}" > "${LIST_RESOLVED}"

echo "::: override-kernel: generating CPIO -> ${CPIO_DST}"
"${GEN_INIT_CPIO}" "${LIST_RESOLVED}" > "${CPIO_DST}"
ls -l "${CPIO_DST}"

# Touch ensures the next make picks up the new file; the bin2vri target
# (patch 0010) rewraps Image into Image.vri.
echo "::: override-kernel: rebuilding Image.vri with new cpio embedded"
touch "${CPIO_DST}"
make -C "${KSRC}" ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" \
    WERROR=0 -j"$(nproc)" Image.vri

echo "::: override-kernel: done"
