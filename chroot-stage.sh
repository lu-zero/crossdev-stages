#!/bin/bash

gentoo_arch() {
    local os_arch=$1
    case $os_arch in
        x86_64) ARCH=amd64 FLAVOR=amd64-openrc;;
        aarch64) ARCH=arm64 FLAVOR=arm64-openrc;;
        riscv*) ARCH=riscv FLAVOR=rv64_lp64d-openrc;;
        *) ARCH=$os_arch FLAVOR=$ARCH-openrc;;
    esac
# echo "$os_arch => $ARCH"
}

get_stage() {
    gentoo_arch $1
    STAGE="stage3-$FLAVOR"
    BASE_URL="https://distfiles.gentoo.org/releases/$ARCH/autobuilds/"
    LATEST_URL="$BASE_URL/latest-$STAGE.txt"
    STAGE3_FILE=$(curl $LATEST_URL -s -f | grep -B1 'BEGIN PGP SIGNATURE' | head -n 1 | cut -d\  -f 1)
    STAGE3_URL="$BASE_URL/$STAGE3_FILE"

    echo Unpacking $STAGE3_FILE

    mkdir -p $2
    curl $STAGE3_URL | tar --exclude 'dev/*' -xJpf - -C $2
}

run_bwrap() {
    local where=$1
    shift
    local args=$@

    sudo bwrap \
        --bind $where / \
        --dev-bind /dev dev \
        --proc /proc \
        --bind /sys sys \
        --ro-bind /etc/resolv.conf etc/resolv.conf \
        --hostname gentoo \
        --unshare-uts \
        $args
}

usage() {
    echo "$0 <setup|enter> <chroot-dir> [arch] "
    echo "$0 run <chroot-dir> <cmd> [args..]"
    echo "setup a gentoo chroot and use it through bubblewrap"
    exit 1
}

[[ -z "$2" ]] && usage

case $1 in
    setup)
        shift
        [[ -z $2 ]] && arch=`uname -m` || arch=$2
        get_stage $arch $1
        ;;
    enter)
        shift
        run_bwrap $1 bash
        ;;
    run)
        shift
        run_bwrap $@
        ;;
    *)
        usage
        ;;
esac
