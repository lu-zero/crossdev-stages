#!/bin/bash

# Common library for crossdev-stages scripts

# Load configuration from file
load_config() {
    local config_file="$1"
    if [[ -f "$config_file" ]]; then
        # shellcheck source=/dev/null
        source "$config_file"
        echo "Loaded configuration from $config_file"
    else
        echo "Error: Configuration file $config_file not found"
        exit 1
    fi
}

# Get Gentoo architecture mapping
gentoo_arch() {
    local os_arch=$1
    case $os_arch in
        x86_64) ARCH=amd64 FLAVOR=amd64-openrc;;
        aarch64) ARCH=arm64 FLAVOR=arm64-openrc;;
        riscv*) ARCH=riscv FLAVOR=rv64_lp64d-openrc;;
        *) ARCH=$os_arch FLAVOR=$ARCH-openrc;;
    esac
}

# Run command in bubblewrap container
run_bwrap() {
    local where=$1
    shift
    local args=$@

    sudo bwrap \
        --bind "$where" / \
        --dev-bind /dev dev \
        --proc /proc \
        --bind /sys sys \
        --ro-bind /etc/resolv.conf etc/resolv.conf \
        --hostname gentoo \
        --clearenv \
        --setenv TERM "$TERM" \
        --setenv COLORTERM "$COLORTERM" \
        --setenv NO_COLOR "$NO_COLOR" \
        --setenv HOME /root \
        --unshare-uts \
        $args
}

# Check if running as root
check_root() {
    if [[ `whoami` != "root" ]]; then
        echo "This script requires root"
        exit 1
    fi
}

# Read package list from file
read_package_list() {
    local file=$1
    if [[ -f "$file" ]]; then
        grep -v '^#' "$file" | grep -v '^$' | tr '\n' ' '
    else
        echo "Warning: Package list file $file not found"
    fi
}

# Setup crossdev environment
setup_crossdev_env() {
    local root="${CROSSDEV_ROOT}"

    echo "Setting up crossdev environment for ${TARGET_CHOST}"

    # Initialize crossdev
    crossdev "${TARGET_CHOST}" --init-target

    # Set profile
    PORTAGE_CONFIGROOT="${CROSSDEV_ROOT}" eselect profile set "${GENTOO_PROFILE}"

    # Configure make.conf
    sed -i -e "s:CFLAGS=.*:CFLAGS=\"${CFLAGS}\":" "${CROSSDEV_MAKE_CONF}"
    echo 'LLVM_TARGETS="AArch64 RISCV"' >> "${root}/etc/portage/make.conf"

    # Setup directories
    mkdir -p "${root}/etc/portage/env"
    echo 'CFLAGS="-O3 -pipe"' >> "${root}/etc/portage/env/plain.conf"
    echo 'CXXFLAGS="-O3 -pipe"' >> "${root}/etc/portage/env/plain.conf"

    mkdir "${root}/etc/portage/package.env"
    echo "dev-lang/rust plain.conf" > "${root}/etc/portage/package.env/rust"

    mkdir -p "${root}/etc/portage/package.{use,accept_keywords}"
    echo -e ">=virtual/libcrypt-2-r1 static-libs\n>=sys-libs/libxcrypt-4.4.36-r3 static-libs\n>=sys-apps/busybox-1.36.1-r3 -pam static" > "${root}/etc/portage/package.use/busybox"
    echo "llvm-core/clang -extra" > "${root}/etc/portage/package.use/clang"
    echo "dev-lang/rust rustfmt -system-llvm" > "${root}/etc/portage/package.use/rust"

    # Workaround crossdev unmasking improperly
    mkdir -p /etc/portage/package.{accept_keywords,mask}
    echo "cross-${TARGET_CHOST}/rust-std **" > /etc/portage/package.accept_keywords/rust-std
    echo "=cross-${TARGET_CHOST}/gcc-15*" > /etc/portage/package.mask/cross-${TARGET_CHOST}-fixup

    # The new meson-based build system tries to run run iconv tests
    echo "dev-vcs/git -iconv" > "${root}/etc/portage/package.use/git"

    mkdir "${CROSSDEV_ROOT}/bin"

    # crossdev starts as split_usr layout
    merge-usr --root "${CROSSDEV_ROOT}"

    # Install crossdev packages
    crossdev "${TARGET_CHOST}" --g "${GCC_VERSION}" --ex-pkg sys-devel/clang-crossdev-wrappers --ex-pkg sys-devel/rust-std

    # Add gcc-16 prereleases
    echo "<sys-devel/gcc-16.0.9999:16 **" > "${root}/etc/portage/package.accept_keywords/gcc"
}

# Prepare stage1 environment
prepare_stage1() {
    local root=$1

    mkdir -p "${root}/etc/portage/"
    cp -a "/usr/${TARGET_CHOST}/etc/portage/{make.profile,profile}" "${root}/etc/portage/"
    echo "CHOST=${TARGET_CHOST}" > "${root}/etc/portage/make.conf"
    echo "ACCEPT_KEYWORDS=~${TARGET_KEYWORD}" >> "${root}/etc/portage/make.conf"
    echo "CFLAGS=\"${CFLAGS}\"" >> "${root}/etc/portage/make.conf"
    echo 'CXXFLAGS=$CFLAGS' >> "${root}/etc/portage/make.conf"
    PORTAGE_CONFIGROOT="${root}" eselect profile set "${GENTOO_PROFILE}"
}

# Install stage1 packages
install_stage1() {
    local root=$1
    local stage1_packages

    # Load stage1 packages from Gentoo's default list
    if [[ -f "/var/db/repos/gentoo/profiles/default/linux/packages.build" ]]; then
        stage1_packages=$(grep -v '#' /var/db/repos/gentoo/profiles/default/linux/packages.build)
    else
        echo "Warning: Could not find Gentoo's default packages.build, using fallback"
        stage1_packages=$(read_package_list "${STAGE1_PACKAGES_FILE}")
    fi

    ROOT="$root" USE=build "${TARGET_CHOST}"-emerge -k -b baselayout
    ROOT="$root" "${TARGET_CHOST}"-emerge -k -b ${stage1_packages}
    ROOT="$root" USE=build "${TARGET_CHOST}"-emerge -k -b portage
}

# Update ldconfig
update_ldconfig() {
    local stage_dir=$1
    ldconfig -v -r "$stage_dir"
}

# Checkout git repository
checkout_repo() {
    local repo=$1
    local tag=$2
    local dest=$3

    if [[ -d "$dest" ]]; then
        (cd "$dest" && git fetch && git checkout "$tag")
    else
        git clone --depth 1 --branch "$tag" "$repo" "$dest"
    fi
}

# Show usage information
show_usage() {
    echo "Usage: $0 <command> [options]"
    echo "Try '$0 --help' for more information"
}

# Parse command line arguments
parse_args() {
    while [[ $# -gt 0 ]]; do
        case $1 in
            --help|-h)
                show_usage
                exit 0
                ;;
            --config|-c)
                CONFIG_FILE="$2"
                shift 2
                ;;
            --platform|-p)
                PLATFORM="$2"
                shift 2
                ;;
            --verbose|-v)
                VERBOSE=1
                shift
                ;;
            --*)
                echo "Unknown option: $1"
                show_usage
                exit 1
                ;;
            *)
                break
                ;;
        esac
    done
}
