# Copyright 2026 Sungjoon Moon
# Distributed under the terms of the Apache-2.0 License

EAPI=8

inherit git-r3

DESCRIPTION="SpaceMIT coprocessor firmware (RT-Thread / ESOS) — K1 N308 or K3 RT24"
HOMEPAGE="https://github.com/openeuler-riscv/spacemit-k3-firmware-esos"
EGIT_REPO_URI="https://github.com/openeuler-riscv/spacemit-k3-firmware-esos"
EGIT_BRANCH="spacemit-k3-esos"

LICENSE="Apache-2.0"
SLOT="0"
KEYWORDS=""
IUSE="k1 k3"
REQUIRED_USE="^^ ( k1 k3 )"

BDEPEND="
	dev-embedded/riscv64-elf-gcc[multilib]
	sys-apps/dtc
	dev-util/scons
	dev-embedded/u-boot-tools
	k3? ( sys-apps/lzop )
"
RDEPEND="k3? ( sys-firmware/esos-lite )"

S="${WORKDIR}/${P}"

src_prepare() {
	eapply "${FILESDIR}"/01-upstream-toolchain.patch
	if use k3; then
		cp "${FILESDIR}"/libc_shim.c bsp/spacemit/applications/ || die
		mkdir -p bsp/binary bsp/spacemit/binary || die
		cp "${EROOT}"/lib/firmware/esos_lite.bin bsp/binary/ || die
		cp "${EROOT}"/lib/firmware/esos_lite.bin bsp/spacemit/binary/ || die
	fi
	eapply_user
}

src_configure() {
	export RTT_EXEC_PATH=/usr/bin
	export RTT_CC_PREFIX=riscv64-elf-
	if use k1; then
		# 0=n308 chip, 0=k1-x board
		printf '0\n0\n' | ./build.sh config || die "k1 config failed"
	fi
	# K3 reconfigures per-OS in src_compile.
}

src_compile() {
	export RTT_EXEC_PATH=/usr/bin
	export RTT_CC_PREFIX=riscv64-elf-
	if use k1; then
		./build.sh || die "k1 build failed"
	else
		mkdir -p ../output/esos || die
		printf '1\n0\n' | ./build.sh config || die "k3 OS0 config"
		./build.sh                          || die "k3 OS0 build"
		printf '1\n1\n' | ./build.sh config || die "k3 OS1 config"
		./build.sh clean
		./build.sh                          || die "k3 OS1 build"
		./build.sh itb                      || die "k3 FIT pack"
	fi
}

src_install() {
	if use k1; then
		insinto /lib/firmware
		newins bsp/spacemit/rtthread-n308.elf esos.elf
	else
		insinto /boot
		doins bsp/spacemit/esos.itb
	fi
}
