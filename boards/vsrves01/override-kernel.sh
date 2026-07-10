#!/bin/bash
# override-kernel.sh for VSRVES01.
#
# Architecture: phram-rootfs.  Linux on this SoC cannot reach the SD
# card directly — the SD bus + controller IP are physically owned by
# the VSDSP6 DSP (datasheet block diagram + RV register map §3.3.3
# ends at GPIO).  The full Gentoo userland ships as a compressed
# squashfs that vendor DDRLoad copies to DDR at boot (see
# override-assemble.sh) and the kernel mounts via phram — squashfs
# decompresses 4 KiB blocks on demand, so ~80 MiB of files cost
# ~25-40 MiB of RAM on this board's 128 MiB DRAM (a cpio embed would
# extract everything to ramfs up front and OOM).
#
# Stages:
#
#   0  hard-reset kernel tree to ${KERNEL_TAG} and re-apply patches.
#   1  build kernel with a placeholder empty catboard.cpio so all Make
#      targets succeed; compile the host gen_init_cpio.
#   2  cross-emerge -e @system into /target with our CFLAGS, then
#      cross-emerge target-packages.txt.  Apply initramfs-overlay,
#      strip, enable agetty.ttyUL0.
#   3  mksquashfs /target -> image.sqfs (staged onto the FAT by
#      override-assemble.sh; no cpio embed, kernel is final at stage 1).

set -euo pipefail

KSRC=/build/linux
SCRIPTS=/scripts/boards/vsrves01
TARGET_SYSROOT=/target
SQFS_IMG="${KSRC}/image.sqfs"
TARGET_PKGS="${SCRIPTS}/target-packages.txt"
INITRAMFS_OVERLAY="${SCRIPTS}/initramfs-overlay"
CPIO_DST="${KSRC}/catboard.cpio"
GEN_INIT_CPIO="${KSRC}/usr/gen_init_cpio"

SQFS_COMP="${SQFS_COMP:-lz4}"

CHOST="${CROSS_COMPILE%-}"
STRIP="${STRIP:-${CHOST}-strip}"

# --- Stage 0: apply patches ------------------------------------------------
# Hard reset so patch edits are picked up cleanly on rebuilds.
echo "::: override-kernel: resetting tree to ${KERNEL_TAG} and re-applying patches"
cd "${KSRC}"
git clean -fdxq
git reset --hard "${KERNEL_TAG}" -q
for p in "${SCRIPTS}/patches/"*.patch; do
    git apply --whitespace=nowarn "$p"
done

# --- Stage 1: initial kernel build with placeholder cpio -------------------
echo "::: override-kernel: stage 1 (placeholder cpio)"
: > "${CPIO_DST}"
make ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" "${KERNEL_DEFCONFIG}"
make ARCH="${KERNEL_ARCH}" CROSS_COMPILE="${CROSS_COMPILE}" WERROR=0 -j"$(nproc)" \
    Image.vri vlsi/vsrves01-catboard.dtb modules

# gen_init_cpio is a kbuild hostprog; kbuild skips its rule when
# INITRAMFS_SOURCE points at a pre-made .cpio.  Single C file, no
# kernel headers — compile directly.
echo "::: override-kernel: compiling host gen_init_cpio"
${HOSTCC:-cc} -O2 -o "${GEN_INIT_CPIO}" "${KSRC}/usr/gen_init_cpio.c"

# --- Stage 2: rebuild /target from source with our CFLAGS -----------------
# Stage3 binaries for rv32-musl are built by Gentoo catalyst with the
# profile-default -march=rv32imac (i.e. WITH the C extension).  VSRV1
# has no C — every function in libc.so.1 would trap "Invalid opcode" on
# the SoC.  -e @system rebuilds the entire @system set from source; the
# CFLAGS that win come from /target/etc/portage/{make.conf,env/*}
# written by `target prepare_portage` (Rust side), which carry our
# -march=rv32ima_zicsr_zifencei.
#
# --usepkg=n forces source build (no fallback to RVC-tainted binpkg
# cache).  --keep-going tolerates the few packages that legitimately
# fail to cross-build for rv32 (strace, ...).
# Stage 2a is gated by a per-target marker (NOT the per-build marker
# Rust uses) — survives across `image build` invocations so re-running
# after a downstream failure (mksquashfs, kernel rebuild, ...) doesn't
# re-emerge 504 packages for 90 minutes.  Delete the marker to force a
# fresh -e @system rebuild.
SYS_MARK="${TARGET_SYSROOT}/.system-rebuilt"
if [ ! -f "${SYS_MARK}" ]; then
    echo "::: override-kernel: stage 2a (emerge -e @system into ${TARGET_SYSROOT})"
    # --keep-going lets us tolerate the handful of packages that
    # legitimately fail to cross-build for rv32-musl (net-tools,
    # strace) — they're not in our target-packages.txt and we don't
    # ship them.  emerge still exits non-zero in that case, but the
    # important @system bits are merged.  We verify essentials below
    # before marking done.
    PORTAGE_CONFIGROOT="${TARGET_SYSROOT}" ROOT="${TARGET_SYSROOT}" \
        "${CHOST}-emerge" -e --usepkg=n --keep-going \
        --quiet --color=n @system || \
        echo "::: override-kernel: stage 2a emerge exited non-zero (--keep-going dropped some pkgs)"

    # Essential-files smoke test — the things we actually need on the
    # final image.  If any are missing, stage 2a really failed and we
    # don't mark done.
    for f in usr/sbin/init bin/busybox lib/libc.so usr/sbin/openrc; do
        if [ ! -e "${TARGET_SYSROOT}/$f" ]; then
            echo "!! stage 2a essential missing: $f" >&2
            exit 1
        fi
    done
    touch "${SYS_MARK}"
    echo "::: override-kernel: stage 2a marker written ${SYS_MARK}"
else
    echo "::: override-kernel: stage 2a SKIPPED (marker present: ${SYS_MARK})"
fi

# Unmerge packages that we WANT replaced by a lighter alternative.
# net-misc/dhcpcd in particular: heavy event loop spins the 98 MHz
# rv32 core to ~100% before triggering a 75s watchdog soft lockup on
# the AUI ethernet driver's link-up event.  busybox udhcpc handles
# this in <1s.  Idempotent — re-running is a no-op if already gone.
UNMERGE_LIGHT=(
    net-misc/dhcpcd
)
LIGHT_MARK="${TARGET_SYSROOT}/.lightened"
if [ ! -f "${LIGHT_MARK}" ]; then
    echo "::: override-kernel: stage 2a.5 (unmerge heavy alternatives)"
    PORTAGE_CONFIGROOT="${TARGET_SYSROOT}" ROOT="${TARGET_SYSROOT}" \
        "${CHOST}-emerge" --unmerge --quiet --color=n "${UNMERGE_LIGHT[@]}" || \
        echo "::: override-kernel: unmerge exited non-zero, continuing"
    touch "${LIGHT_MARK}"
fi

# First column only — lines may carry a keyword column ("atom **")
# that the Rust side writes to package.accept_keywords; passing the
# whole line as one argv element makes emerge reject it as an invalid
# atom and abort the entire stage 2b merge.
mapfile -t pkgs < <(grep -Ev '^[[:space:]]*(#|$)' "${TARGET_PKGS}" | awk '{print $1}')
[ ${#pkgs[@]} -gt 0 ] || { echo "!! ${TARGET_PKGS} is empty" >&2; exit 1; }

echo "::: override-kernel: stage 2b (emerge target-packages.txt)"
PORTAGE_CONFIGROOT="${TARGET_SYSROOT}" ROOT="${TARGET_SYSROOT}" \
    "${CHOST}-emerge" --usepkg=n --keep-going \
    --quiet --color=n "${pkgs[@]}" || \
    echo "::: override-kernel: stage 2b emerge exited non-zero (--keep-going dropped some pkgs)"

# --- Stage 2.1: overlay /etc -----------------------------------------------
echo "::: override-kernel: applying overlay ${INITRAMFS_OVERLAY}"
if [ -d "${INITRAMFS_OVERLAY}" ]; then
    cp -a "${INITRAMFS_OVERLAY}/." "${TARGET_SYSROOT}/"
fi

# --- Stage 2.1b: nuke systemd-utils orphans -------------------------------
# stage3 ships systemd-tmpfiles + systemd-hwdb + udev rules under /etc and
# /usr/bin/systemd-* but their PKGDB entries are gone (we masked
# systemd-utils via package.provided so portage can't unmerge them
# cleanly).  They drop into openrc runlevels and panic at boot with
# "Error loading shared library libsystemd-shared-N.so".  Wipe.
echo "::: override-kernel: removing systemd-* orphans"
rm -rf "${TARGET_SYSROOT}/usr/bin/systemd-"* \
       "${TARGET_SYSROOT}/usr/lib/systemd" \
       "${TARGET_SYSROOT}/lib/systemd" \
       "${TARGET_SYSROOT}/etc/cron.daily/systemd-tmpfiles-clean" \
       "${TARGET_SYSROOT}/etc/init.d/systemd-tmpfiles-setup"* \
       "${TARGET_SYSROOT}/etc/runlevels/"*/systemd-tmpfiles-setup* \
       "${TARGET_SYSROOT}/usr/lib/netifrc/sh/systemd-wrapper.sh" \
       "${TARGET_SYSROOT}/usr/share/zsh/site-functions/_systemd-"*

# --- Stage 2.1c: surface gcc-libs runtime libraries -----------------------
# libatomic, libgcc_s, libstdc++ live under /usr/lib/gcc/<chost>/<ver>/
# by default — the dynamic linker doesn't search there.  libcrypto.so.3
# (OpenSSL) needs libatomic.so.1 for 8-byte atomics on rv32 (musl emits
# __atomic_*_8 builtin calls).  Copy the runtime ABIs into the normal
# ilp32 library path so musl's ld can find them.
GCC_LIBDIR="${TARGET_SYSROOT}/usr/lib/gcc/${CHOST}/16"
ILP32_LIBDIR="${TARGET_SYSROOT}/usr/lib/ilp32"
mkdir -p "${ILP32_LIBDIR}"
for lib in libatomic.so libatomic.so.1 libatomic.so.1.2.0 \
           libgcc_s.so libgcc_s.so.1 \
           libstdc++.so libstdc++.so.6 libstdc++.so.6.0.36; do
    if [ -e "${GCC_LIBDIR}/$lib" ] && [ ! -e "${ILP32_LIBDIR}/$lib" ]; then
        cp -a "${GCC_LIBDIR}/$lib" "${ILP32_LIBDIR}/$lib"
    fi
done

# --- Stage 2.2: enable agetty on the vendor UART --------------------------
# openrc ships /etc/init.d/agetty as a service template; named symlinks
# (agetty.ttyUL0) bind it to a specific tty, configured via the
# matching /etc/conf.d/agetty.ttyUL0 (baud=1000000, in the overlay).
# Also whitelist ttyUL0 in /etc/securetty so root can log in there.
ln -sf agetty "${TARGET_SYSROOT}/etc/init.d/agetty.ttyUL0"
mkdir -p "${TARGET_SYSROOT}/etc/runlevels/default"
ln -sf /etc/init.d/agetty.ttyUL0 \
    "${TARGET_SYSROOT}/etc/runlevels/default/agetty.ttyUL0"
grep -qxF ttyUL0 "${TARGET_SYSROOT}/etc/securetty" 2>/dev/null \
    || echo ttyUL0 >> "${TARGET_SYSROOT}/etc/securetty"

# `local` service runs /etc/local.d/*.start at default runlevel; we
# ship a tiny network.start that calls busybox udhcpc (lighter than
# dhcpcd, which spins the rv32 core to 100% for ~75s on link-up).
ln -sf /etc/init.d/local \
    "${TARGET_SYSROOT}/etc/runlevels/default/local"

# sshd at default runlevel.
[ -e "${TARGET_SYSROOT}/etc/init.d/dropbear" ] && \
    ln -sf /etc/init.d/dropbear \
        "${TARGET_SYSROOT}/etc/runlevels/default/dropbear"

# Drop the dhcpcd runlevel symlink if it lingered from a prior build.
rm -f "${TARGET_SYSROOT}/etc/runlevels/default/dhcpcd"

# --- Stage 2.3: strip ELF + drop docs/headers/static-libs -----------------
echo "::: override-kernel: strip"
rm -rf "${TARGET_SYSROOT}/usr/share/doc"  "${TARGET_SYSROOT}/usr/share/man" \
       "${TARGET_SYSROOT}/usr/share/info" "${TARGET_SYSROOT}/usr/share/locale" \
       "${TARGET_SYSROOT}/usr/share/gtk-doc" "${TARGET_SYSROOT}/usr/include"
find "${TARGET_SYSROOT}" -name '*.a' -delete
find "${TARGET_SYSROOT}" -name '*.la' -delete

if command -v "${STRIP}" >/dev/null 2>&1; then
    find "${TARGET_SYSROOT}" -type f \( -name '*.so' -o -name '*.so.*' \) \
        -exec "${STRIP}" --strip-unneeded {} + 2>/dev/null || true
    for d in bin sbin usr/bin usr/sbin; do
        [ -d "${TARGET_SYSROOT}/$d" ] || continue
        find "${TARGET_SYSROOT}/$d" -type f ! -name '*.sh' \
            -exec "${STRIP}" --strip-all {} + 2>/dev/null || true
    done
fi

du -sh "${TARGET_SYSROOT}" 2>/dev/null || true

# --- Stage 3: pack /target into squashfs ----------------------------------
# Excludes: portage state, distfiles, headers, build-time deps that
# have no runtime value.  The squashfs IS the rootfs at runtime —
# /etc/portage etc. are useless on a read-only image.
echo "::: override-kernel: mksquashfs -> ${SQFS_IMG}"
EXFILE=$(mktemp)
cat > "${EXFILE}" <<'EOF'
# Portage / build state.
var/db/pkg
var/db/repos
var/cache
var/lib/portage
usr/include
usr/lib/portage
usr/lib/ilp32/portage
usr/src
etc/portage
etc/eselect

# Docs / man / locale / info.
usr/share/doc
usr/share/man
usr/share/info
usr/share/locale
usr/share/gtk-doc
usr/share/i18n
usr/share/zoneinfo
usr/share/sgml
usr/share/xml

# Toolchain trees (gcc, binutils, llvm, perl, python) — installed by
# stage3 + -e @system but never executed on the device.  Targets both
# /usr/lib and the ilp32 multilib subdir.  Together: 1.4 GiB.
usr/libexec/gcc
usr/lib/gcc
usr/lib/ilp32/gcc
usr/lib/binutils
usr/lib/ilp32/binutils
usr/lib/llvm
usr/lib/ilp32/llvm
usr/lib/cmake
usr/lib/ilp32/cmake
usr/lib/python3.13
usr/lib/python3.14
usr/lib/ilp32/python3.13
usr/lib/ilp32/python3.14
usr/lib/python-exec
usr/lib/ilp32/python-exec
usr/share/python-exec
usr/lib/perl5
usr/lib/ilp32/perl5
usr/share/perl5
usr/x86_64-pc-linux-gnu
usr/riscv32-unknown-linux-musl

# Build / doc tools (autoconf/cmake/groff/texinfo bundles).
usr/share/aclocal
usr/share/aclocal-1.18
usr/share/automake-1.18
usr/share/autoconf-2.72
usr/share/libtool
usr/share/help2man
usr/share/cmake
usr/share/binutils-data
usr/share/gcc-data
usr/share/misc
usr/share/hwdata
usr/share/groff
usr/share/texi2any
usr/share/texinfo
usr/share/gettext

# Host-binary dev tools that landed in /target's $PATH.
usr/bin/python3*
usr/bin/python
usr/bin/perl
usr/bin/perl[0-9]*
usr/bin/cmake
usr/bin/ctest
usr/bin/cpack
usr/bin/ccmake
usr/bin/cmake-gui
usr/bin/meson
usr/bin/ninja
usr/bin/autoconf*
usr/bin/autoreconf*
usr/bin/autoheader*
usr/bin/autoupdate*
usr/bin/automake*
usr/bin/aclocal*
usr/bin/autom4te*
usr/bin/autoscan*
usr/bin/ifnames*
usr/bin/libtool*
usr/bin/libtoolize
usr/bin/m4
usr/bin/make
usr/bin/gmake
usr/bin/bison
usr/bin/yacc
usr/bin/flex
usr/bin/flex++
usr/bin/lex
usr/bin/re2c
usr/bin/help2man
usr/bin/groff
usr/bin/troff
usr/bin/nroff
usr/bin/eqn
usr/bin/tbl
usr/bin/pic
usr/bin/refer
usr/bin/grog*
usr/bin/grotty*
usr/bin/grodvi*
usr/bin/groff*
usr/bin/lookbib
usr/bin/indxbib
usr/bin/eqn2graph
usr/bin/grap2graph
usr/bin/pic2graph
usr/bin/post-grohtml
usr/bin/pre-grohtml
usr/bin/preconv
usr/bin/soelim
usr/bin/tfmtodit
usr/bin/afmtodit
usr/bin/addftinfo
usr/bin/hpftodit
usr/bin/mmroff
usr/bin/pdfmom
usr/bin/pdfroff
usr/bin/info
usr/bin/install-info
usr/bin/makeinfo
usr/bin/texindex
usr/bin/texi2any
usr/bin/pod*
usr/bin/perlbug
usr/bin/perldoc
usr/bin/perlivp
usr/bin/perlthanks
usr/bin/2to3*
usr/bin/idle3*
usr/bin/pydoc3*
usr/bin/pip3*
usr/bin/easy_install*
usr/bin/sqlite3*
usr/bin/gpg*
usr/bin/gpgconf
usr/bin/gpgparsemail
usr/bin/gpgsm
usr/bin/gpgtar
usr/bin/gpgv
usr/bin/dirmngr*
usr/bin/kbxutil
usr/bin/watchgnupg
usr/sbin/sshd
usr/bin/ssh
usr/bin/scp
usr/bin/sftp
usr/bin/ssh-add
usr/bin/ssh-agent
usr/bin/ssh-copy-id
usr/bin/ssh-keygen
usr/bin/ssh-keyscan
usr/libexec/ssh*
usr/libexec/openssh
usr/bin/wget
usr/bin/curl
usr/bin/rsync
usr/bin/gnutls-cli*
usr/bin/gnutls-serv*
usr/bin/certtool
usr/bin/psktool
usr/bin/ocsptool
usr/bin/danetool
usr/bin/srptool
usr/bin/p11tool

# gcc/binutils binary tools under /usr/bin/ (also picked up by the
# generic gcc*/binutils-* patterns above but listed explicitly for the
# multilib-prefixed names like riscv32-unknown-linux-musl-gcc).
usr/bin/gcc
usr/bin/gcc-*
usr/bin/g++
usr/bin/g++-*
usr/bin/cpp
usr/bin/c++
usr/bin/cc-*
usr/bin/cpp-*
usr/bin/ld
usr/bin/ld.bfd
usr/bin/ld.gold
usr/bin/as
usr/bin/ar
usr/bin/nm
usr/bin/ranlib
usr/bin/strip
usr/bin/objcopy
usr/bin/objdump
usr/bin/addr2line
usr/bin/c++filt
usr/bin/elfedit
usr/bin/readelf
usr/bin/size
usr/bin/strings
usr/bin/dwp
usr/bin/gprof
usr/bin/gcov*
usr/bin/lto-dump*
usr/bin/install-xattr
usr/bin/riscv32-unknown-linux-musl-*
usr/bin/x86_64-pc-linux-gnu-*

# Sandbox / portage runtime bits.
usr/lib/sandbox
usr/lib/ilp32/sandbox
usr/share/portage
usr/lib/python-exec/python*

# Misc dev/doc data.
usr/share/gnupg
usr/share/man-db
usr/share/openssh
usr/share/baselayout/share
EOF
rm -f "${SQFS_IMG}"
mksquashfs "${TARGET_SYSROOT}" "${SQFS_IMG}" \
    -comp "${SQFS_COMP}" \
    -noappend -no-progress -no-xattrs -no-exports \
    -ef "${EXFILE}" -wildcards \
    -e '*.a' '*.la'
rm -f "${EXFILE}"
ls -l "${SQFS_IMG}"

# No cpio/initramfs stages — defconfig has CONFIG_BLK_DEV_INITRD=n.
# The Image.vri built in stage 1 is the final kernel; vendor's DDRLoad
# (extended in override-assemble.sh) puts image.sqfs at 0x83000000 and
# the kernel mounts it via phram+squashfs (root= cmdline).

echo "::: override-kernel: done"
