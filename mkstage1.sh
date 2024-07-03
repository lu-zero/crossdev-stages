#!/bin/bash

if [[ `whoami` != "root" ]]; then
    echo "This script requires root"
    exit 1
fi

if [[ -z "$1" ]]; then
    echo "Usage: $0 <stage1-directory>"
    exit 1
fi

STAGE1_DIR=$1

STAGE1_PACKAGES=`grep -v '#' /var/db/repos/gentoo/profiles/default/linux/packages.build`
PROFILE=default/linux/riscv/23.0/rv64/split-usr/lp64d
OUR_CFLAGS="-O3 -march=rv64gcv_zvl256b -pipe"
OUR_CHOST=riscv64-unknown-linux-gnu
OUR_KEYWORD=riscv
CROSSDEV_ROOT=/usr/${OUR_CHOST}
CROSSDEV_MAKE_CONF=${CROSSDEV_ROOT}/etc/portage/make.conf
OPTS="-j50 --load-average 100"


setup_crossdev() {
    crossdev riscv64-unknown-linux-gnu
    PORTAGE_CONFIGROOT=${CROSSDEV_ROOT} eselect profile set ${PROFILE}
    sed -i -e "s:CFLAGS=.*:CFLAGS=\"${OUR_CFLAGS}\":" ${CROSSDEV_MAKE_CONF}
}

build_packages() {
    export EMERGE_DEFAULT_OPTS="$OPTS"
    export MAKEOPTS="$OPTS"
    riscv64-unknown-linux-gnu-emerge -b ${STAGE1_PACKAGES}
}

prepare_stage1() {
    local root=$1

    mkdir -p ${root}/etc/portage/
    echo "CHOST=${OUR_CHOST}" > ${root}/etc/portage/make.conf
    echo "ACCEPT_KEYWORDS=~$OUR_KEYWORD" >> ${root}/etc/portage/make.conf
    echo "CFLAGS=\"$OUR_CFLAGS\"" >> ${root}/etc/portage/make.conf
    echo 'CXXFLAGS=$CFLAGS' >> ${root}/etc/portage/make.conf
    PORTAGE_CONFIGROOT=${root} eselect profile set ${PROFILE}
}

install_stage1() {
    export EMERGE_DEFAULT_OPTS="$OPTS"
    export MAKEOPTS="$OPTS"
    export CFLAGS="$OUR_CFLAGS --sysroot=${STAGE1_DIR}"
    ROOT=$1 USE=build riscv64-unknown-linux-gnu-emerge -k baselayout
    ROOT=$1 riscv64-unknown-linux-gnu-emerge -k ${STAGE1_PACKAGES}
    ROOT=$1 USE=build riscv64-unknown-linux-gnu-emerge portage
}

setup_crossdev
build_packages
prepare_stage1 $STAGE1_DIR
install_stage1 $STAGE1_DIR
