# Copyright 2026 Sungjoon Moon
# Distributed under the terms of the Apache-2.0 License

EAPI=8

inherit meson

MY_P="${PN}-v${PV}"

DESCRIPTION="Alpine Package Keeper, the package manager of Alpine Linux"
HOMEPAGE="https://gitlab.alpinelinux.org/alpine/apk-tools"
SRC_URI="${HOMEPAGE}/-/archive/v${PV}/${MY_P}.tar.gz"
S="${WORKDIR}/${MY_P}"

# apk-tools itself is GPL-2.0-only (SPDX header on every source file).
# libfetch/ is vendored from FreeBSD and linked into libapk.
LICENSE="GPL-2 BSD"
SLOT="0"
KEYWORDS="~amd64 ~arm ~arm64 ~riscv ~x86"
IUSE="test +zstd"
RESTRICT="!test? ( test )"

# What the built binary links, checked with ldd rather than assumed:
# libssl, libcrypto, libz, and libzstd when USE=zstd.  libfetch is
# vendored, so there is no URL-backend dependency of any kind.
RDEPEND="
	dev-libs/openssl:=
	sys-libs/zlib:=
	zstd? ( app-arch/zstd:= )
"
DEPEND="${RDEPEND}"
# scdoc renders the man pages, lua generates the built-in help database
# and the shell completions.  genhelp.lua's zlib module is optional: it
# pcall()s the require and falls back to piping through gzip, so no lua
# binding is needed, only the interpreter.
BDEPEND="
	app-text/scdoc
	dev-lang/lua:5.4
	test? ( dev-util/cmocka )
"

src_configure() {
	local emesonargs=(
		-Dcrypto_backend=openssl
		-Ddocs=enabled
		-Dhelp=enabled
		-Dlua_version=5.4
		# libapk's lua and python bindings; nothing here consumes them.
		-Dlua=disabled
		-Dpython=disabled
		-Durl_backend=libfetch
		$(meson_feature test tests)
		$(meson_feature zstd)
	)
	# No -Darch: src/apk_arch.h derives the compiled-in default from the
	# compiler's own target.  It names the Alpine arch, not the libc, so on
	# glibc it still says "x86_64"; harmless, because every caller that
	# populates a foreign root passes --arch explicitly.
	meson_src_configure
}
