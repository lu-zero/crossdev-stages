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

BDEPEND="
	dev-embedded/riscv64-elf-gcc[multilib]
	sys-apps/dtc
	dev-util/scons
	dev-embedded/u-boot-tools
"

S="${WORKDIR}/${P}/rt-thread"

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
