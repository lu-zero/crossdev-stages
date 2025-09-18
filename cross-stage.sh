#!/bin/bash

if [[ `whoami` != "root" ]]; then
    echo "This script requires root"
    exit 1
fi

usage() {
    echo "Usage: $0 <command> <stage-directory>"
    echo
    echo "make   : Create a new stage1"
    echo "update : Update a pre-existing stage3"
    echo
    echo "install_clang : Install clang in the stage"
    echo "install_boot  : Install the booloader requirements"
    echo "install_more  : Install additional starting packages"
    exit 1
}

STAGE_DIR=$2

STAGE1_PACKAGES=`grep -v '#' /var/db/repos/gentoo/profiles/default/linux/packages.build`
ADDITIONAL_PACKAGES="
  sys-block/parted
  net-wireless/wpa_supplicant
  app-editors/vim
  app-admin/metalog
  net-misc/ntp
  dev-vcs/git
  sys-devel/mold
  dev-lang/go
  dev-build/cmake
  dev-lang/rust
  net-wireless/iw
  app-misc/screen
  sys-process/htop
  net-analyzer/nmap
  app-portage/gentoolkit
  app-portage/genlop
"
# sys-apps/ripgrep tries to execute itself on install.

# Building rust requires more manual changes

PROFILE=default/linux/riscv/23.0/rv64/lp64d
# Please report bugs and link them to https://gcc.gnu.org/bugzilla/show_bug.cgi?id=116242
# GCC_VER=16.0.9999
GCC_VER=16.0.0_p20250907
OUR_CFLAGS="-O3 -march=rv64gcv_zvl256b -pipe"
#OUR_CFLAGS="-O3 -pipe"
OUR_CHOST=riscv64-unknown-linux-gnu
OUR_KEYWORD=riscv
CROSSDEV_ROOT=/usr/${OUR_CHOST}
CROSSDEV_MAKE_CONF=${CROSSDEV_ROOT}/etc/portage/make.conf
OPTS="-j50 --load-average 100"
export EMERGE_DEFAULT_OPTS="$OPTS"
export MAKEOPTS="$OPTS"
export FEATURES="parallel-install -merge-wait"

setup_crossdev() {
    local root=${CROSSDEV_ROOT}
    crossdev riscv64-unknown-linux-gnu --init-target
    PORTAGE_CONFIGROOT=${CROSSDEV_ROOT} eselect profile set ${PROFILE}
    sed -i -e "s:CFLAGS=.*:CFLAGS=\"${OUR_CFLAGS}\":" ${CROSSDEV_MAKE_CONF}
    echo 'LLVM_TARGETS="AArch64 RISCV"' >> ${root}/etc/portage/make.conf
    mkdir -p ${root}/etc/portage/env
    echo 'CFLAGS="-O3 -pipe"' >> ${root}/etc/portage/env/plain.conf
    echo 'CXXFLAGS="-O3 -pipe"' >> ${root}/etc/portage/env/plain.conf
    mkdir ${root}/etc/portage/package.env
    echo "dev-lang/rust plain.conf" > ${root}/etc/portage/package.env/rust
    mkdir -p ${root}/etc/portage/package.{use,accept_keywords}
    echo -e '>=virtual/libcrypt-2-r1 static-libs\n>=sys-libs/libxcrypt-4.4.36-r3 static-libs\n>=sys-apps/busybox-1.36.1-r3 -pam static' > ${root}/etc/portage/package.use/busybox
    echo "llvm-core/clang -extra" > ${root}/etc/portage/package.use/clang
    echo "dev-lang/rust rustfmt -system-llvm" > ${root}/etc/portage/package.use/rust
    # Workaround crossdev unmasking improperly
    mkdir -p /etc/portage/package.{accept_keywords,mask}
    echo "cross-riscv64-unknown-linux-gnu/rust-std **" > /etc/portage/package.accept_keywords/rust-std
    echo "=cross-riscv64-unknown-linux-gnu/gcc-15*" > /etc/portage/package.mask/cross-riscv64-unknown-linux-gnu-fixup
    # The new meson-based build system tries to run run iconv tests
    echo "dev-vcs/git -iconv" > ${root}/etc/portage/package.use/git
    echo 'CFLAGS="-O3 -march=rv64gc -pipe"' > ${root}/etc/portage/env/rv64gc
    echo "dev-libs/libgcrypt rv64gc" >${root}/etc/portage/package.env/libgcrypt
    mkdir ${CROSSDEV_ROOT}/bin
    # crossdev starts as split_usr layout
    merge-usr --root ${CROSSDEV_ROOT}
    crossdev riscv64-unknown-linux-gnu --g $GCC_VER --ex-pkg sys-devel/clang-crossdev-wrappers --ex-pkg sys-devel/rust-std
    # Add gcc-16 prereleases
    echo "<sys-devel/gcc-16.0.9999:16 **" > ${root}/etc/portage/package.accept_keywords/gcc
}

prepare_stage1() {
    local root=$1

    mkdir -p ${root}/etc/portage/
    cp -a /usr/$OUR_CHOST/etc/portage/{make.profile,profile} ${root}/etc/portage/
    echo "CHOST=${OUR_CHOST}" > ${root}/etc/portage/make.conf
    echo "ACCEPT_KEYWORDS=~$OUR_KEYWORD" >> ${root}/etc/portage/make.conf
    echo "CFLAGS=\"$OUR_CFLAGS\"" >> ${root}/etc/portage/make.conf
    echo 'CXXFLAGS=$CFLAGS' >> ${root}/etc/portage/make.conf
    PORTAGE_CONFIGROOT=${root} eselect profile set ${PROFILE}
}

install_stage1() {
    ROOT=$1 USE=build riscv64-unknown-linux-gnu-emerge -k -b baselayout
    ROOT=$1 riscv64-unknown-linux-gnu-emerge -k -b ${STAGE1_PACKAGES}
    ROOT=$1 USE=build riscv64-unknown-linux-gnu-emerge -k -b portage
}

install_perl() {
    local root=${CROSSDEV_ROOT}
#    echo 'LDFLAGS="$LDFLAGS --sysroot=$EROOT"' > ${root}/etc/portage/env/override-sysroot
#    echo "dev-lang/perl override-sysroot" >${root}/etc/portage/package.env/perl
    riscv64-unknown-linux-gnu-emerge perl
    ROOT=$1 riscv64-unknown-linux-gnu-emerge perl
}

update_stage3() {
    riscv64-unknown-linux-gnu-emerge -b -k gcc
    riscv64-unknown-linux-gnu-emerge -b -k sys-libs/binutils-libs
    riscv64-unknown-linux-gnu-emerge -b -k -u system
    ROOT=$1 riscv64-unknown-linux-gnu-emerge -k -e @world
}

install_clang() {
    # clang-tidy fails to cross-build
    # TODO: make so plugin-api.h exists even w/out emerging this again
    riscv64-unknown-linux-gnu-emerge -b -k sys-libs/binutils-libs
    ROOT=$1 riscv64-unknown-linux-gnu-emerge -b -k llvm-core/clang
}

install_boot() {
    # dracut and busybox must be installed on host and target
    ROOT=$1 riscv64-unknown-linux-gnu-emerge busybox dracut
}

install_more() {
    ROOT=$1 riscv64-unknown-linux-gnu-emerge -b -k $ADDITIONAL_PACKAGES
}

maybe_prepare() {
    if [[ -e ${CROSSDEV_ROOT} ]]
    then
        echo 'Crossdev already present, use `prepare` to regenerate'
    else
        echo "Creating a new crossdev environment for ${OUR_CHOST}"
        setup_crossdev
    fi
}

update_ldconfig() {
    # ldconfig -v -C /etc/ld.so.cache -f $STAGE_DIR/etc/ld.so.conf -r $STAGE_DIR
    ldconfig -v -C /etc/ld.so.cache -r $STAGE_DIR
}

if [[ -z "$1" ]]; then
    usage
fi

case $1 in
    prepare)
        setup_crossdev
        exit 0
        ;;
esac

if [[ -z "$2" ]]; then
    usage
fi

case $1 in
    make)
        maybe_prepare
        prepare_stage1 $STAGE_DIR
        install_stage1 $STAGE_DIR
        ;;
    update)
        maybe_prepare
        update_stage3 $STAGE_DIR
        update_ldconfig
        ;;
    update_ldconfig)
        update_ldconfig
        ;;
    install_clang)
        maybe_prepare
        install_clang $STAGE_DIR
        update_ldconfig
        ;;
    install_boot)
        maybe_prepare
        install_boot $STAGE_DIR
        ;;
    install_more)
        maybe_prepare
        install_more $STAGE_DIR
        update_ldconfig
        ;;
    install_perl)
        maybe_prepare
        install_perl $STAGE_DIR
        ;;
    *)
        usage
        ;;
esac
