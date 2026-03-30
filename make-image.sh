#!/bin/bash

. /etc/profile

BASE_DIR=$(dirname $(readlink -f "$0"))
source "$BASE_DIR/lib/board.sh"

usage() {
    echo "Usage: $0 [--dry-run] <board> <build-directory> <stage-directory>"
    echo
    echo "Boards: $(list_boards | tr '\n' ' ')"
    exit 1
}

DRY_RUN=0
if [[ "$1" == "--dry-run" ]]; then
    DRY_RUN=1
    shift
fi

if [[ -z "$3" ]]; then
    usage
fi

BOARD=$1
BUILD_DIR=$2
STAGE_DIR=$3

load_board "$BOARD" || exit 1
BOARD_DIR="$_BOARD_BASE_DIR/boards/$BOARD"
IMAGE_NAME="$BOARD_IMAGE_NAME"
export CROSS_COMPILE="${BOARD_ARCH}-unknown-linux-gnu-"
export ARCH=riscv
export OPENSBI="$BUILD_DIR/opensbi/build/platform/${BOARD_OPENSBI_PLATFORM}/firmware/${BOARD_OPENSBI_BINARY}"

# Source board.sh for optional overrides (board_boot, board_root, etc.)
if [[ -f "$BOARD_DIR/board.sh" ]]; then
    source "$BOARD_DIR/board.sh"
fi

if [[ $DRY_RUN -eq 1 ]]; then
    echo "Board:      $BOARD"
    echo "Build dir:  $BUILD_DIR"
    echo "Stage dir:  $STAGE_DIR"
    echo "Board dir:  $BOARD_DIR"
    echo "Image name: $IMAGE_NAME"
    echo "Steps:      ${BOARD_BUILD_STEPS[*]}"
    echo "OPENSBI:    $OPENSBI"
    echo "Repos:      ${BOARD_REPO_NAMES[*]}"
    for step in "${BOARD_BUILD_STEPS[@]}"; do
        if type -t "board_${step}" &>/dev/null; then
            echo "  $step: board override"
        else
            echo "  $step: default"
        fi
    done
    exit 0
fi

# --- Default step implementations ---

checkout() {
    local repo=$1
    local tag=$2
    local src=$BUILD_DIR/$3
    if [[ -d $src ]]; then
        (cd $src && git fetch && git checkout "$tag")
    else
        git clone --depth 1 --branch $tag $repo $src
    fi
}

default_checkout() {
    mkdir -p "$BUILD_DIR"
    local i
    for ((i = 0; i < ${#BOARD_REPO_NAMES[@]}; i++)); do
        checkout "${BOARD_REPO_URLS[$i]}" "${BOARD_REPO_TAGS[$i]}" "${BOARD_REPO_NAMES[$i]}"
    done
}

default_bootloader() {
    local extra="${BOARD_OPENSBI_EXTRA//\{build_dir\}/$BUILD_DIR}"
    pushd $BUILD_DIR
    eval make -C opensbi PLATFORM="${BOARD_OPENSBI_PLATFORM}" $extra -j$(nproc)
    make -C u-boot "${BOARD_UBOOT_DEFCONFIG}"
    make -C u-boot -j$(nproc)
    popd
}

default_linux() {
    pushd $BUILD_DIR
    make -C linux "${BOARD_LINUX_DEFCONFIG}"
    local target
    for target in "${BOARD_LINUX_EXTRA_TARGETS[@]}"; do
        [[ -n "$target" ]] && make -C linux "$target" -j$(nproc)
    done
    make -C linux -j$(nproc)
    make -C linux modules -j$(nproc)
    popd
}

setup_service() {
    local service=$1
    local runlevel=$2
    local root=$BUILD_DIR/gen/root
    ln -sf /etc/init.d/$1 $root/etc/runlevels/$2/
}

default_root() {
    local root=$BUILD_DIR/gen/root
    mkdir -p $root
    cp -a $STAGE_DIR/* $root
    INSTALL_MOD_PATH=$root make -C $BUILD_DIR/linux modules_install
    make -C $BUILD_DIR/linux/tools/perf V=1 WERROR=0 DESTDIR=$(pwd)/$root/usr/ install
    setup_service sshd default
    setup_service metalog default
    setup_service swclock boot
    setup_service ntpd default
    cat > $root/etc/fstab << 'EOF'
LABEL=rootfs	/	ext4	defaults	0 1
LABEL=bootfs	/boot	ext4	defaults	0 2
EOF
    mkdir -p $root/var/lib/misc
    touch $root/var/lib/misc/lastclock
    echo 'hostname="gentoo"' > $root/etc/conf.d/hostname
    echo "x1:12345:respawn:/sbin/agetty 115200 console linux" >> $root/etc/inittab
    sed -i -e 's/root:x:/root::/' $root/etc/passwd
    echo "PermitRootLogin yes" >> $root/etc/ssh/sshd_config
    echo "PermitEmptyPasswords yes" >> $root/etc/ssh/sshd_config
    echo "StrictModes yes" >> $root/etc/ssh/sshd_config
    ldconfig -v -r $root
}

default_boot() {
    echo "Error: No board_boot() defined for $BOARD" >&2
    echo "Add board_boot() to $BOARD_DIR/board.sh" >&2
    exit 1
}

default_image() {
    pushd $BUILD_DIR
    rm -fR $BUILD_DIR/tmp
    genimage --config $BOARD_DIR/genimage.cfg
    xz -f -T0 -9 $IMAGE_NAME
    popd
}

# --- Pipeline: TOML defines order, board.sh overrides steps ---

for step in "${BOARD_BUILD_STEPS[@]}"; do
    if type -t "board_${step}" &>/dev/null; then
        echo ">>> $step (board)"
        "board_${step}"
    else
        echo ">>> $step"
        "default_${step}"
    fi
done
