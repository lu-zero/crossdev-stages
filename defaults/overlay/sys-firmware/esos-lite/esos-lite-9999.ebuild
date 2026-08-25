# Copyright 2026 Sungjoon Moon
# Distributed under the terms of the Apache-2.0 License

EAPI=8

inherit git-r3

DESCRIPTION="K3 RT24 PM mini-firmware blob, incbin'd into sys-firmware/esos[k3]"
HOMEPAGE="https://github.com/openeuler-riscv/spacemit-k3-firmware-esos-lite"
EGIT_REPO_URI="https://github.com/openeuler-riscv/spacemit-k3-firmware-esos-lite"
EGIT_BRANCH="spacemit-k3-esos-lite"

LICENSE="Apache-2.0"
SLOT="0"
KEYWORDS=""

# Cross-compiled firmware blob input: keep the host strip away.
RESTRICT="strip"

# The bare-metal toolchain comes from crossdev (cross-riscv64-elf/*),
# whose generated atoms don't exist until the user runs it — checked in
# pkg_setup instead of a dependency atom.
BDEPEND="
	sys-apps/dtc
	dev-build/scons
	dev-embedded/u-boot-tools
"

S="${WORKDIR}/${P}/rt-thread"

pkg_setup() {
	type -P riscv64-elf-gcc >/dev/null ||
		die "riscv64-elf-gcc not found; run: crossdev -t riscv64-elf -s4"
}

src_prepare() {
	# Patch generated against repo root; strip the "rt-thread/" prefix to apply at S.
	eapply -p2 "${FILESDIR}"/01-march-underscore.patch
	eapply_user
}

src_configure() {
	export RTT_EXEC_PATH=/usr/bin
	export RTT_CC_PREFIX=riscv64-elf-
	printf '0\n' | ./build.sh config || die "config failed"
}

src_compile() {
	export RTT_EXEC_PATH=/usr/bin
	export RTT_CC_PREFIX=riscv64-elf-
	./build.sh || die "build failed"
}

src_install() {
	insinto /lib/firmware
	doins bsp/spacemit/esos_lite.bin
}
