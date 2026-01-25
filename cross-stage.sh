#!/bin/bash

# Cross-stage script - refactored to use external configuration

# Source common library
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/lib/common.sh"

# Default configuration
DEFAULT_PLATFORM="riscv64-k1"
DEFAULT_CONFIG="config/platforms/${DEFAULT_PLATFORM}.conf"

# Help function
display_help() {
    echo "Usage: $0 [options] <command> [stage-directory]"
    echo
    echo "Options:"
    echo "  --config,-c <file>  Use alternative configuration file"
    echo "  --platform,-p <name> Use specific platform configuration"
    echo "  --help,-h           Show this help message"
    echo
    echo "Commands:"
    echo "  prepare             Setup crossdev environment"
    echo "  make               Create a new stage1"
    echo "  update             Update a pre-existing stage3"
    echo "  update_ldconfig    Update ldconfig cache"
    echo "  install_clang      Install clang in the stage"
    echo "  install_boot       Install the bootloader requirements"
    echo "  install_more       Install additional starting packages"
    echo "  install_perl       Install perl"
    echo
    echo "Examples:"
    echo "  $0 --help                          Show this help"
    echo "  $0 prepare                         Setup crossdev environment"
    echo "  $0 make /path/to/stage             Create a new stage1"
    echo "  $0 --platform riscv64-k1 make /path/to/stage"
    exit 0
}

# Parse command line arguments (but preserve non-option arguments)
TEMP_ARGS=()
while [[ $# -gt 0 ]]; do
    case $1 in
        --help|-h)
            display_help
            exit 0
            ;;
        --config|-c)
            CONFIG_FILE="$2"
            shift 2
            ;;
        --platform|-p)
            PLATFORM="$2"
            if [[ -n "$PLATFORM" ]]; then
                CONFIG_FILE="config/platforms/${PLATFORM}.conf"
            fi
            shift 2
            ;;
        --verbose|-v)
            VERBOSE=1
            shift
            ;;
        --*)
            echo "Unknown option: $1"
            usage
            exit 1
            ;;
        *)
            TEMP_ARGS+=("$1")
            shift
            ;;
    esac
done

# Restore non-option arguments
set -- "${TEMP_ARGS[@]}"

# Load configuration
CONFIG_FILE="${CONFIG_FILE:-$DEFAULT_CONFIG}"
load_config "$CONFIG_FILE"

# Set global variables from config
CROSSDEV_ROOT="/usr/${TARGET_CHOST}"
CROSSDEV_MAKE_CONF="${CROSSDEV_ROOT}/etc/portage/make.conf"
OPTS="-j50 --load-average 100"
export EMERGE_DEFAULT_OPTS="$OPTS"
export MAKEOPTS="$OPTS"
export FEATURES="parallel-install -merge-wait"

# Check root (but allow --help to work without root)
if [[ "$1" != "--help" && "$1" != "-h" ]]; then
    check_root
fi

setup_crossdev() {
    setup_crossdev_env
}

prepare_stage1() {
    prepare_stage1 "$1"
}

install_stage1() {
    install_stage1 "$1"
}

install_perl() {
    local stage_dir=$1
    local root=${CROSSDEV_ROOT}
#    echo 'LDFLAGS="$LDFLAGS --sysroot=$EROOT"' > ${root}/etc/portage/env/override-sysroot
#    echo "dev-lang/perl override-sysroot" >${root}/etc/portage/package.env/perl
    "${TARGET_CHOST}"-emerge perl
    ROOT="$stage_dir" "${TARGET_CHOST}"-emerge perl
}

update_stage3() {
    local stage_dir=$1
    "${TARGET_CHOST}"-emerge -b -k gcc
    "${TARGET_CHOST}"-emerge -b -k sys-libs/binutils-libs
    "${TARGET_CHOST}"-emerge -b -k -u system
    ROOT="$stage_dir" "${TARGET_CHOST}"-emerge -k -e @world
}

install_clang() {
    local stage_dir=$1
    # clang-tidy fails to cross-build
    # TODO: make so plugin-api.h exists even w/out emerging this again
    "${TARGET_CHOST}"-emerge -b -k sys-libs/binutils-libs
    ROOT="$stage_dir" "${TARGET_CHOST}"-emerge -b -k llvm-core/clang
}

install_boot() {
    local stage_dir=$1
    # dracut and busybox must be installed on host and target
    ROOT="$stage_dir" "${TARGET_CHOST}"-emerge busybox dracut
}

install_more() {
    local stage_dir=$1
    local additional_packages
    
    additional_packages=$(read_package_list "${ADDITIONAL_PACKAGES_FILE}")
    ROOT="$stage_dir" "${TARGET_CHOST}"-emerge -b -k ${additional_packages}
}

maybe_prepare() {
    if [[ -e ${CROSSDEV_ROOT} ]]
    then
        echo 'Crossdev already present, use `prepare` to regenerate'
    else
        echo "Creating a new crossdev environment for ${TARGET_CHOST}"
        setup_crossdev
    fi
}

update_ldconfig() {
    update_ldconfig "$STAGE_DIR"
}

if [[ -z "$1" ]]; then
    display_help
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
        display_help
        ;;
esac
