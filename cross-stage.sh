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
    exit 1
}

if [[ -z "$2" ]]; then
    usage
fi

STAGE_DIR=$2

STAGE1_PACKAGES=`grep -v '#' /var/db/repos/gentoo/profiles/default/linux/packages.build`
PROFILE=default/linux/riscv/23.0/rv64/lp64d
# Until https://gcc.gnu.org/bugzilla/show_bug.cgi?id=115789 is fixed we cannot reliably using vectors
# OUR_CFLAGS="-O3 -march=rv64gcv_zvl256b -pipe"
OUR_CFLAGS="-O3 -pipe"
OUR_CHOST=riscv64-unknown-linux-gnu
OUR_KEYWORD=riscv
CROSSDEV_ROOT=/usr/${OUR_CHOST}
CROSSDEV_MAKE_CONF=${CROSSDEV_ROOT}/etc/portage/make.conf
OPTS="-j50 --load-average 100"
export EMERGE_DEFAULT_OPTS="$OPTS"
export MAKEOPTS="$OPTS"

setup_crossdev() {
    crossdev riscv64-unknown-linux-gnu
    PORTAGE_CONFIGROOT=${CROSSDEV_ROOT} eselect profile set ${PROFILE}
    sed -i -e "s:CFLAGS=.*:CFLAGS=\"${OUR_CFLAGS}\":" ${CROSSDEV_MAKE_CONF}
    mkdir -p ${CROSSDEV_ROOT}/etc/portage/env
    mkdir ${CROSSDEV_ROOT}/etc/portage/package.env
    # crossdev starts as split_usr layout
    mkdir ${CROSSDEV_ROOT}/bin
    merge-usr --root ${CROSSDEV_ROOT}
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
    echo "LDFLAGS=\"\$LDFLAGS --sysroot=$STAGE_DIR\"" > ${CROSSDEV_ROOT}/etc/portage/env/override-sysroot.conf
    echo "dev-lang/perl override-sysroot.conf" > ${CROSSDEV_ROOT}/etc/portage/package.env/perl
    ROOT=$1 USE=build riscv64-unknown-linux-gnu-emerge -k -b baselayout
    ROOT=$1 riscv64-unknown-linux-gnu-emerge -k -b ${STAGE1_PACKAGES}
    ROOT=$1 USE=build riscv64-unknown-linux-gnu-emerge -k -b portage
}

update_stage3() {
    rm -f ${CROSSDEV_ROOT}/etc/portage/package.env/perl
    ROOT=$1 riscv64-unknown-linux-gnu-emerge -u @world
}

install_clang() {
    rm -f ${CROSSDEV_ROOT}/etc/portage/package.env/perl
    # clang-tidy fails to cross-build
    # using clang to build compiler-rt requires clang existing and having
    # the {target}-clang symlinks
    USE="-extra -clang" ROOT=$1 riscv64-unknown-linux-gnu-emerge clang
}

if [[ -e ${CROSSDEV_ROOT} ]]
then
    echo "Crossdev already present, rm -fR ${CROSSDEV_ROOT} to regenerate"
else
    echo "Creating a new crossdev environment for ${OUR_CHOST}"
    setup_crossdev
fi

case $1 in
    make)
        prepare_stage1 $STAGE_DIR
        install_stage1 $STAGE_DIR
        ;;
    update)
        update_stage3 $STAGE_DIR
        ;;
    install_clang)
        install_clang $STAGE_DIR
        ;;
    *)
        usage
        ;;
esac
