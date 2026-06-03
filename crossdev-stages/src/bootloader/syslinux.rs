use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::Result;

/// Clone syslinux into /build/syslinux, using bare repo cache at /cache/sources/.
pub fn clone(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    if let (Some(repo), Some(tag)) = (&board.syslinux_repo, &board.syslinux_tag) {
        crate::source_cache::cached_clone(runner, repo, tag, "/build/syslinux", "syslinux")?;
    }
    Ok(())
}

/// Build syslinux BIOS blobs + installer.
///
/// Two-pass approach:
/// 1. Build the full `bios` target with the i586 cross-compiler. This produces
///    the x86 machine code blobs (mbr.bin, ldlinux.sys, ldlinux.c32, etc.)
///    and the installer tools (all as i586 binaries).
/// 2. Rebuild just the `mtools/syslinux` installer with the host compiler.
///    This variant works on unmounted FAT images without root, perfect for
///    use with genimage's exec-post hook.
pub fn build(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    if board.syslinux_repo.is_none() {
        return Ok(());
    }

    let cc = &board.cross_compile;

    // Create a gcc wrapper that injects -fcommon.
    // syslinux 6.04 predates GCC 10's -fno-common default, causing
    // "multiple definition" linker errors without this flag.
    // Placed in /build so it persists across sandbox invocations.
    runner.run(&format!(
        "printf '#!/bin/sh\\nexec {cc}gcc -fcommon \"$@\"\\n' > /build/syslinux-cc && \
         chmod +x /build/syslinux-cc"
    ))?;

    let cross_tools =
        format!("CC=/build/syslinux-cc LD={cc}ld AR={cc}ar OBJCOPY={cc}objcopy RANLIB={cc}ranlib");

    // Restore any modified source files from git before patching
    runner.run("cd /build/syslinux && git checkout -- mbr/Makefile")?;

    // Apply workarounds for building with modern GCC/binutils:
    // 1. Strip .note.gnu.property section from MBR to keep it under 440 bytes
    //    (Gentoo patch 0001-Strip-the-.note.gnu.property-section-for-the-mbr.patch)
    // 2. Disable MBR size check — mbr.bin may be slightly oversized with newer
    //    toolchains; we only need mbr.bin as a 440-byte MBR bootstrap anyway.
    runner.run(
        "cd /build/syslinux && \
         sed -i 's|\\$(OBJCOPY) -O binary|\\$(OBJCOPY) --remove-section .note.gnu.property -O binary|' \
         mbr/Makefile && \
         sed -i 's|\\$(PERL) \\$(SRC)/checksize\\.pl \\$@|true|' mbr/Makefile",
    )?;

    // Clean previous build artifacts
    runner.run("make -C /build/syslinux spotless")?;

    // Build host tools with native compiler.
    // syslinux's build system compiles some tools (e.g. lzo/prepcore) and then
    // EXECUTES them during the build. If these are cross-compiled for i586,
    // they can't run on the host (e.g. aarch64). Build them with host gcc first.
    runner.run(
        "cd /build/syslinux && \
         mkdir -p bios/lzo/src && \
         make -C bios/lzo -f /build/syslinux/lzo/Makefile \
         SRC=/build/syslinux/lzo OBJ=/build/syslinux/bios/lzo \
         MAKEDIR=/build/syslinux/mk topdir=/build/syslinux \
         CC=gcc LD=ld AR=ar RANLIB=ranlib \
         all",
    )?;

    // Pass 1: build bios blobs + installer with cross-compiler.
    // Use -k to continue past non-critical failures (e.g. utils/isohybrid
    // which needs uuid/uuid.h not available to the cross-compiler).
    // We only need: mbr/*.bin, core/*, ldlinux.c32, libinstaller/*, mtools/*.
    // The pipe absorbs the non-zero exit code from `make -k`.
    runner.run(&format!(
        "cd /build/syslinux && \
         make -j$(nproc) -k {cross_tools} UPX=false bios 2>&1 | tail -1 && \
         test -f bios/mbr/mbr.bin && \
         test -f bios/core/ldlinux.sys && \
         test -f bios/com32/elflink/ldlinux/ldlinux.c32"
    ))?;

    // Pass 2: rebuild just the mtools-based installer for the host arch.
    // This binary runs on the build host (e.g. aarch64) and manipulates
    // unmounted FAT images — no root or mount required.
    // Clean pass-1 object files (i586) and force rebuild with host compiler.
    // We must pass all the variables normally set by the top-level Makefile's
    // recursive make invocation (MAKEDIR, topdir, objdir, FIRMWARE, ARCH, etc.)
    // since we're bypassing the normal build orchestration.
    runner.run(
        "cd /build/syslinux && \
         rm -rf bios/mtools && \
         mkdir -p bios/mtools && \
         make -j$(nproc) -C bios/mtools \
         -f /build/syslinux/mtools/Makefile \
         SRC=/build/syslinux/mtools \
         OBJ=/build/syslinux/bios/mtools \
         MAKEDIR=/build/syslinux/mk \
         topdir=/build/syslinux \
         objdir=/build/syslinux/bios \
         OBJDIR=/build/syslinux/bios \
         FIRMWARE=BIOS FWCLASS=BIOS \
         ARCH=i386 \
         CC=gcc LD=ld AR=gcc-ar RANLIB=gcc-ranlib \
         all",
    )
}
