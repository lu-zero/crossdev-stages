#! /bin/bash
# More structured sandbox prototype
#
# - cache the stage3 in a .cache path
# - unpack either in a known place in $CACHE_DIR/sandboxes/ or as needed
# - provide an enter/run command that by default uses the latest sandbox

CACHE_DIR="${HOME}/.cache/crossdev-stages"
STAGES_DIR="${CACHE_DIR}/stages"
SANDBOXES_DIR="${CACHE_DIR}/sandboxes"
TARGETS_DIR="${CACHE_DIR}/targets"

ensure_cache_dirs() {
    mkdir -p "$STAGES_DIR"
    mkdir -p "$SANDBOXES_DIR"
    mkdir -p "$TARGETS_DIR"
}

get_latest_sandbox() {
    local latest_sandbox=""
    if [[ -d "$SANDBOXES_DIR" ]]; then
        latest_sandbox=$(ls -t "$SANDBOXES_DIR" | head -n 1)
        if [[ -n "$latest_sandbox" ]]; then
            echo "$SANDBOXES_DIR/$latest_sandbox"
            return 0
        fi
    fi
    echo "" >&2
    return 1
}

get_latest_target() {
    local latest_target=""
    if [[ -d "$TARGETS_DIR" ]]; then
        latest_target=$(ls -t "$TARGETS_DIR" | head -n 1)
        if [[ -n "$latest_target" ]]; then
            echo "$TARGETS_DIR/$latest_target"
            return 0
        fi
    fi
    echo "" >&2
    return 1
}

# Helper function to set or replace a variable in make.conf
set_make_conf_var() {
    local file="$1"
    local var_name="$2"
    local var_value="$3"

    if grep -q "^${var_name}=" "$file"; then
        sed -i "s|^${var_name}=.*|${var_name}=\"${var_value}\"|" "$file"
    else
        echo "${var_name}=\"${var_value}\"" >> "$file"
    fi
}

configure_portage() {
    local sandbox_dir="$1"
    local arch="$2"
    gentoo_arch $arch

    # Detect CPU count
    local cpu_count=$(nproc 2>/dev/null || echo 4)
    local p=$((cpu_count / 2 + 1))
    local q="$cpu_count"

    # Create portage directories
    mkdir -p "$sandbox_dir/etc/portage"
    mkdir -p "$sandbox_dir/etc/portage/package.accept_keywords"

    # Configure make.conf - append to existing or create new
    local make_conf="$sandbox_dir/etc/portage/make.conf"

    # Add or update Portage settings (replace existing values)
    if [[ ! -f "$make_conf" ]]; then
        cat > "$make_conf" << EOF
MAKEOPTS="-j${p} --load-average ${q}"
EMERGE_DEFAULT_OPTS="--jobs=${p} --load-average ${q}"
FEATURES="parallel-install -merge-wait"
ACCEPT_KEYWORDS="~${ARCH}"
EOF
    else
        # Use helper function to set/replace variables
        set_make_conf_var "$make_conf" "MAKEOPTS" "-j${p} --load-average ${q}"
        set_make_conf_var "$make_conf" "EMERGE_DEFAULT_OPTS" "--jobs=${p} --load-average ${q}"
        set_make_conf_var "$make_conf" "FEATURES" "parallel-install -merge-wait"
        set_make_conf_var "$make_conf" "ACCEPT_KEYWORDS" "~${ARCH}"
    fi

    # Set LLVM_TARGETS for cross-compilation
#    local llvm_target=$(llvm_arch "$arch")
#    if [[ -n "$llvm_target" ]]; then
#        set_make_conf_var "$make_conf" "LLVM_TARGETS" "$llvm_target"
#    fi

    echo "Portage configured for ${ARCH} in $sandbox_dir"
}

# Setup cross-compilation environment within sandbox
setup_crossdev_sandbox() {
    local sandbox_dir="$1"
    local target_arch="$2"

    # Map target architecture to Gentoo variables
    gentoo_arch "$target_arch"
    local chost="${target_arch}-unknown-linux-gnu"

    # Fix profile path - need to handle riscv specially
    local profile=""
    case "$ARCH-$FLAVOR" in
        riscv-rv64_lp64d-openrc)
            profile="default/linux/riscv/23.0/rv64/lp64d"
            ;;
        *)
            profile="default/linux/${ARCH}/23.0/${FLAVOR}"
            ;;
    esac

    local crossdev_root="/usr/${chost}"
    local crossdev_make_conf="${crossdev_root}/etc/portage/make.conf"
    local gcc_ver="16.0.1_p20260315"
    local cflags="-O3 -march=rv64gc -pipe"

    echo "Setting up crossdev environment for ${chost} in sandbox..."

    # Create the crossdev overlay
    run "$sandbox_dir" eselect repository create crossdev

    # Initialize crossdev for target architecture
    run "$sandbox_dir" crossdev "${chost}" --init-target

    # Add rust-std workaround
    run "$sandbox_dir" "echo \"cross-${target_arch}-unknown-linux-gnu/rust-std **\" > /etc/portage/package.accept_keywords/rust-std"

    # Set up portage profile
    run "$sandbox_dir" "export PORTAGE_CONFIGROOT=${crossdev_root}; eselect profile set ${profile}"

    # Configure CFLAGS in make.conf
    run "$sandbox_dir" sed -i -e "s:CFLAGS=.*:CFLAGS=\"${cflags}\":" "${crossdev_make_conf}"

    # Set LLVM_TARGETS using our llvm_arch function
    local llvm_target=$(llvm_arch "$target_arch")
    if [[ -n "$llvm_target" ]]; then
        run "$sandbox_dir" sh -c "echo \"LLVM_TARGETS=\\\"${llvm_target}\\\"\" >> ${crossdev_make_conf}"
    fi

    # Create portage environment directories
    run "$sandbox_dir" mkdir -p "${crossdev_root}/etc/portage/env"
    run "$sandbox_dir" sh -c "echo 'CFLAGS=\"-O3 -pipe\"' >> ${crossdev_root}/etc/portage/env/plain.conf"
    run "$sandbox_dir" sh -c "echo 'CXXFLAGS=\"-O3 -pipe\"' >> ${crossdev_root}/etc/portage/env/plain.conf"

    # Create package.env directory
    run "$sandbox_dir" mkdir -p "${crossdev_root}/etc/portage/package.env"
    run "$sandbox_dir" sh -c "echo \"dev-lang/rust plain.conf\" > ${crossdev_root}/etc/portage/package.env/rust"

    # Create package.use and package.accept_keywords directories
    run "$sandbox_dir" mkdir -p "${crossdev_root}/etc/portage/package.{use,accept_keywords}"

    # Configure busybox, clang, and rust package settings
    run "$sandbox_dir" sh -c "cat > ${crossdev_root}/etc/portage/package.use/busybox << 'EOF'
>=virtual/libcrypt-2-r1 static-libs
>=sys-libs/libxcrypt-4.4.36-r3 static-libs
>=sys-apps/busybox-1.36.1-r3 -pam static
EOF"

    run "$sandbox_dir" sh -c "echo \"llvm-core/clang -extra\" > ${crossdev_root}/etc/portage/package.use/clang"
    run "$sandbox_dir" sh -c "echo \"dev-lang/rust rustfmt -system-llvm\" > ${crossdev_root}/etc/portage/package.use/rust"

    # Apply workarounds
    run "$sandbox_dir" mkdir -p "/etc/portage/package.{accept_keywords,mask}"

    # Git iconv workaround
    run "$sandbox_dir" sh -c "echo \"dev-vcs/git -iconv\" > ${crossdev_root}/etc/portage/package.use/git"

    # Run merge-usr
    run "$sandbox_dir" merge-usr --root "${crossdev_root}"

    # Install crossdev with specific GCC version
    run "$sandbox_dir" crossdev "${chost}" --g "${gcc_ver}" --ex-pkg sys-devel/clang-crossdev-wrappers --ex-pkg sys-devel/rust-std

    # Add gcc-16 prereleases
    run "$sandbox_dir" sh -c "echo \"<sys-devel/gcc-16.0.9999:16 **\" > ${crossdev_root}/etc/portage/package.accept_keywords/gcc"

    echo "Crossdev environment setup complete for ${chost}"
}

install_dependencies() {
    local sandbox_dir="$1"

    echo "Installing host system dependencies in sandbox..."

    # Host system dependencies from README.md (with categories)
    local bin_packages=(
        "app-arch/zstd"
        "app-arch/bzip2"
        "app-arch/xz-utils"
    )

    local packages=(
        "sys-devel/crossdev"
        "sys-apps/merge-usr"
        "dev-vcs/git"
        "dev-embedded/u-boot-tools"
        "sys-apps/dtc"
        "sys-kernel/dracut"
        "sys-apps/busybox"
        "sys-fs/genimage"
        "app-eselect/eselect-repository"
        "dev-lang/rust"
    )

    echo "Running getuto..."
    run "$sandbox_dir" getuto || echo "getuto failed, continuing anyway..."

    echo "Emerging bin dependencies..."
    run "$sandbox_dir" emerge -G "${bin_packages[@]}" || echo "Some packages failed to emerge"
    echo "Emerging dependencies..."
    run "$sandbox_dir" emerge -b -k "${packages[@]}" || echo "Some packages failed to emerge"

    echo "Installing Rust ldconfig..."
    run "$sandbox_dir" cargo install --root /usr/local ldconfig

    echo "Host dependencies installation complete"
}

prepare_sandbox() {
    local sandbox_dir="$1"
    local arch="$2"

    echo "Preparing sandbox $sandbox_dir for architecture $arch..."

    # Configure Portage settings
    configure_portage "$sandbox_dir" "$arch"

    # Install host dependencies
    install_dependencies "$sandbox_dir"

    echo "Sandbox preparation complete for $arch"
}

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

# Map OS architecture to LLVM target for LLVM_TARGETS variable
llvm_arch() {
    local os_arch=$1
    local llvm_target=""

    case $os_arch in
        x86*) llvm_target="X86" ;;
        arm*) llvm_target="ARM" ;;
        aarch64*) llvm_target="AArch64" ;;
        riscv*) llvm_target="RISCV" ;;
        mips*) llvm_target="Mips" ;;
        loongarch*) llvm_target="LoongArch" ;;
        powerpc*) llvm_target="PowerPC" ;;
        sparc*) llvm_target="Sparc" ;;
    esac

    echo "$llvm_target"
}

fetch_stage() {
    local arch=$1
    gentoo_arch $arch
    STAGE="stage3-$FLAVOR"
    BASE_URL="https://distfiles.gentoo.org/releases/$ARCH/autobuilds/"
    LATEST_URL="$BASE_URL/latest-$STAGE.txt"
    STAGE3_FILE=$(curl $LATEST_URL -s -f | grep -B1 'BEGIN PGP SIGNATURE' | head -n 1 | cut -d\  -f 1)
    STAGE3_URL="$BASE_URL/$STAGE3_FILE"

    echo "Fetching $STAGE3_FILE"

    ensure_cache_dirs

    local stage_filename=$(basename "$STAGE3_FILE")
    local stage_file_path="$STAGES_DIR/$stage_filename"

    if [[ ! -f "$stage_file_path" ]]; then
        curl -L "$STAGE3_URL" -o "$stage_file_path" || {
            echo "Failed to download $STAGE3_FILE" >&2
            return 1
        }
    else
        echo "$stage_filename already cached"
    fi

    echo "$stage_file_path"
}



# We use hakoniwa even here to preserve the owners
unpack_stage() {
    local stage_file="$1"
    local sandbox_name="$2"
    local sandbox_dir="$SANDBOXES_DIR/$sandbox_name"
    local stage_filename=$(basename "$stage_file")

    ensure_cache_dirs

    if [[ -d "$sandbox_dir" ]]; then
        echo "Sandbox $sandbox_name already exists"
        echo "$sandbox_dir"
        return 0
    fi

    echo "Creating sandbox $sandbox_name from $stage_file"

    hakoniwa run \
      --rootfs / --devfs /dev \
      --unshare-all \
      --allow-new-privs \
      --userns=auto \
      --tmpfs /tmp \
      -B "$CACHE_DIR":/cache \
      -- /bin/sh -c "
        mkdir -p \"/cache/sandboxes/$sandbox_name\" &&
        tar --overwrite -xpvf \"/cache/stages/$stage_filename\" \
          --xattrs-include='*.*' \
          --numeric-owner \
          --exclude='./dev' \
          -C \"/cache/sandboxes/$sandbox_name\" &&
        echo \"/cache/sandboxes/$sandbox_name\"
      "
}

run() {
    local sandbox_dir=${1}
    shift
    local args=$@

    hakoniwa run \
      --rootdir "$sandbox_dir":rw \
      --devfs /dev \
      -b /etc/resolv.conf \
      --unshare-all \
      --allow-new-privs \
      --userns=auto \
      --network=host \
      --tmpfs /tmp \
      -e TERM="$TERM" \
      -e COLORTERM="$COLORTERM" \
      -e NO_COLOR="$NO_COLOR" \
      -e HOME=/root \
      -- bash --login -c "
         $args
      "
}

run_with_stage() {
    local sandbox_dir="$1"
    local stage_dir="$2"
    shift 2
    local args=$@

    hakoniwa run \
      --rootdir "$sandbox_dir":rw \
      --devfs /dev \
      -b /etc/resolv.conf \
      --unshare-all \
      --allow-new-privs \
      --userns=auto \
      --network=host \
      --tmpfs /tmp \
      -B "$stage_dir":/target:rw \
      -e TERM="$TERM" \
      -e COLORTERM="$COLORTERM" \
      -e NO_COLOR="$NO_COLOR" \
      -e HOME=/root \
      -- bash --login -c "
         $args
      "
}

unpack_target() {
    local stage_file="$1"
    local target_name="$2"
    local target_dir="$TARGETS_DIR/$target_name"
    local stage_filename=$(basename "$stage_file")

    ensure_cache_dirs

    if [[ -d "$target_dir" ]]; then
        echo "Target $target_name already exists"
        echo "$target_dir"
        return 0
    fi

    echo "Creating target $target_name from $stage_file"

    hakoniwa run \
      --rootfs / --devfs /dev \
      --unshare-all \
      --allow-new-privs \
      --userns=auto \
      --tmpfs /tmp \
      -B "$CACHE_DIR":/cache \
      -- /bin/sh -c "
        mkdir -p \"/cache/targets/$target_name\" &&
        tar --overwrite -xpvf \"/cache/stages/$stage_filename\" \
          --xattrs-include='*.*' \
          --numeric-owner \
          --exclude='./dev' \
          -C \"/cache/targets/$target_name\" &&
        echo \"/cache/targets/$target_name\"
      "
}

update_ldconfig_sandbox() {
    local sandbox_dir="$1"
    local stage_dir="$2"

    echo "Updating ld.so.cache in target..."
    run_with_stage "$sandbox_dir" "$stage_dir" "ldconfig -v -r /target"
}

update_stage3() {
    local sandbox_dir="$1"
    local stage_dir="$2"
    local target_arch="${3:-riscv64}"
    local chost="${target_arch}-unknown-linux-gnu"

    echo "Updating stage3 at $stage_dir for ${chost}..."
    run_with_stage "$sandbox_dir" "$stage_dir" "${chost}-emerge -b -k gcc"
    run_with_stage "$sandbox_dir" "$stage_dir" "${chost}-emerge -b -k sys-libs/binutils-libs"
    run_with_stage "$sandbox_dir" "$stage_dir" "${chost}-emerge -b -k -u system"
    run_with_stage "$sandbox_dir" "$stage_dir" "ROOT=/target ${chost}-emerge -k -e @world"
    update_ldconfig_sandbox "$sandbox_dir" "$stage_dir"
}

install_packages() {
    local sandbox_dir="$1"
    local stage_dir="$2"
    local target_arch="$3"
    shift 3
    local chost="${target_arch}-unknown-linux-gnu"

    echo "Installing packages into target for ${chost}..."
    run_with_stage "$sandbox_dir" "$stage_dir" "ROOT=/target ${chost}-emerge -b -k $*"
    update_ldconfig_sandbox "$sandbox_dir" "$stage_dir"
}

packages_from_file() {
    local file="$1"
    grep -v '#' "$file" | grep -v '^[[:space:]]*$'
}

usage() {
    echo "$0 <command> [options]"
    echo ""
    echo "Sandbox commands:"
    echo "  $0 setup [arch] [name]          - Setup sandbox for arch (default: host arch, name: arch)"
    echo "  $0 prepare [sandbox]            - Prepare sandbox with Portage config and host dependencies"
    echo "  $0 setup-crossdev [sandbox] [target-arch] - Setup cross-compilation environment in sandbox"
    echo "  $0 enter [sandbox]             - Enter interactive shell in sandbox (default: latest)"
    echo "  $0 run <sandbox> <cmd>         - Run command in specified sandbox"
    echo ""
    echo "Target commands:"
    echo "  $0 target list                 - List unpacked targets"
    echo "  $0 target setup [arch] [name]  - Setup target sysroot for arch"
    echo "  $0 target update [sandbox] [target] [arch] - Update target via cross-emerge"
    echo "  $0 target pack [target] [arch] - Pack target as stage3 tarball in stages cache"
    echo ""
    echo "Package install commands:"
    echo "  $0 install [sandbox] [target] [arch] pkg... - Install packages into target"
    echo "  $0 install-from [sandbox] [target] [arch] file - Install packages from file"
    echo "  $0 update-ldconfig [sandbox] [target] - Regenerate ld.so.cache in target"
    echo ""
    echo "Cache directory: $CACHE_DIR"
    exit 1
}

main() {
    local cmd="$1"
    shift

    case $cmd in
        setup)
            local arch=""
            if [[ -n "$1" ]]; then
                arch="$1"
                shift
            else
                arch=$(uname -m)
            fi

            local sandbox_name="$arch"
            if [[ -n "$1" ]]; then
                sandbox_name="$1"
                shift
            fi

            echo "Setting up sandbox: $sandbox_name"
            local stage_file=$(fetch_stage "$arch") || exit 1
            local sandbox_dir=$(unpack_stage "$stage_file" "$sandbox_name") || exit 1
            echo "Sandbox ready: $sandbox_dir"
            ;;
        prepare)
            local sandbox_dir=""
            if [[ -n "$1" && "$1" != "latest" ]]; then
                sandbox_dir="$SANDBOXES_DIR/$1"
                shift
            else
                sandbox_dir=$(get_latest_sandbox)
                if [[ -z "$sandbox_dir" ]]; then
                    echo "Error: No sandbox found. Please run setup first." >&2
                    exit 1
                fi
            fi

            if [[ ! -d "$sandbox_dir" ]]; then
                echo "Error: Sandbox not found: $sandbox_dir" >&2
                exit 1
            fi

            # Detect architecture from sandbox name or use default
            local arch=""
            if [[ "$sandbox_dir" == *"$SANDBOXES_DIR/"* ]]; then
                arch=$(basename "$sandbox_dir")
            else
                arch=$(uname -m)
            fi

            prepare_sandbox "$sandbox_dir" "$arch"
            ;;
        setup-crossdev)
            local sandbox_dir=""
            local target_arch=""
            if [[ -n "$1" && "$1" != "latest" ]]; then
                sandbox_dir="$SANDBOXES_DIR/$1"
                shift
            else
                sandbox_dir=$(get_latest_sandbox)
                [[ -n "$1" ]] && shift
            fi

            if [[ ! -d "$sandbox_dir" ]]; then
                echo "Error: No sandbox found. Please run setup first." >&2
                exit 1
            fi

            if [[ -n "$1" ]]; then
                target_arch="$1"
                shift
            else
                # Default to sandbox architecture
                if [[ "$sandbox_dir" == *"$SANDBOXES_DIR/"* ]]; then
                    target_arch=$(basename "$sandbox_dir")
                else
                    target_arch=$(uname -m)
                fi
            fi

            setup_crossdev_sandbox "$sandbox_dir" "$target_arch"
            ;;
        enter)
            local sandbox_dir=""
            if [[ -n "$1" && "$1" != "latest" ]]; then
                sandbox_dir="$SANDBOXES_DIR/$1"
            else
                sandbox_dir=$(get_latest_sandbox)
                if [[ -z "$sandbox_dir" ]]; then
                    echo "Error: No sandbox found. Please run setup first." >&2
                    exit 1
                fi
            fi

            if [[ ! -d "$sandbox_dir" ]]; then
                echo "Error: No sandbox found. Please run setup first." >&2
                exit 1
            fi

            run "$sandbox_dir" bash --login
            ;;
        run)
            local sandbox_dir=""
            if [[ "$1" == "latest" ]]; then
                sandbox_dir=$(get_latest_sandbox)
                if [[ -z "$sandbox_dir" ]]; then
                    echo "Error: No sandbox found. Please run setup first." >&2
                    exit 1
                fi
                shift
            else
                sandbox_dir="$SANDBOXES_DIR/$1"
                shift
            fi

            if [[ ! -d "$sandbox_dir" ]]; then
                echo "Error: Sandbox not found: $sandbox_dir" >&2
                exit 1
            fi

            run "$sandbox_dir" "$@"
            ;;
        target)
            local subcmd="${1:-list}"
            shift

            case $subcmd in
                list)
                    if [[ -d "$TARGETS_DIR" ]]; then
                        ls -lt "$TARGETS_DIR" | tail -n +2
                    else
                        echo "No targets directory found."
                    fi
                    ;;
                setup)
                    local arch=""
                    if [[ -n "$1" ]]; then
                        arch="$1"; shift
                    else
                        arch=$(uname -m)
                    fi
                    local target_name="${1:-$arch}"
                    [[ -n "$1" ]] && shift

                    local stage_file
                    stage_file=$(fetch_stage "$arch") || exit 1
                    unpack_target "$stage_file" "$target_name"
                    ;;
                update)
                    local sandbox_dir=""
                    if [[ -z "$1" || "$1" == "latest" ]]; then
                        sandbox_dir=$(get_latest_sandbox)
                        [[ -z "$sandbox_dir" ]] && { echo "Error: No sandbox found." >&2; exit 1; }
                        [[ -n "$1" ]] && shift
                    else
                        sandbox_dir="$SANDBOXES_DIR/$1"; shift
                    fi

                    local target_dir=""
                    if [[ -z "$1" || "$1" == "latest" ]]; then
                        target_dir=$(get_latest_target)
                        [[ -z "$target_dir" ]] && { echo "Error: No target found." >&2; exit 1; }
                        [[ -n "$1" ]] && shift
                    else
                        target_dir="$TARGETS_DIR/$1"; shift
                    fi

                    local target_arch="${1:-riscv64}"
                    [[ -n "$1" ]] && shift

                    [[ ! -d "$sandbox_dir" ]] && { echo "Error: Sandbox not found: $sandbox_dir" >&2; exit 1; }
                    [[ ! -d "$target_dir" ]] && { echo "Error: Target not found: $target_dir" >&2; exit 1; }

                    update_stage3 "$sandbox_dir" "$target_dir" "$target_arch"
                    ;;
                pack)
                    local target_dir=""
                    if [[ -z "$1" || "$1" == "latest" ]]; then
                        target_dir=$(get_latest_target)
                        [[ -z "$target_dir" ]] && { echo "Error: No target found." >&2; exit 1; }
                        [[ -n "$1" ]] && shift
                    else
                        target_dir="$TARGETS_DIR/$1"; shift
                    fi

                    local target_arch="${1:-riscv64}"
                    [[ -n "$1" ]] && shift
                    gentoo_arch "$target_arch"

                    local timestamp
                    timestamp=$(date +%Y%m%dT%H%M%SZ)
                    local out_file="$STAGES_DIR/stage3-${FLAVOR}-${timestamp}.tar.xz"

                    echo "Packing $target_dir -> $out_file"
                    hakoniwa run \
                      --rootfs / --devfs /dev \
                      --unshare-all \
                      --allow-new-privs \
                      --userns=auto \
                      --tmpfs /tmp \
                      -B "$CACHE_DIR":/cache \
                      -B "$target_dir":/target:ro \
                      -- /bin/sh -c "
                        tar -cpf \"/cache/stages/stage3-${FLAVOR}-${timestamp}.tar.xz\" \
                          --xattrs-include='*.*' \
                          --numeric-owner \
                          --xz \
                          --exclude='./dev' \
                          -C /target .
                      "
                    echo "Packed: $out_file"
                    ;;
                *)
                    usage
                    ;;
            esac
            ;;
        install)
            local sandbox_dir=""
            if [[ -z "$1" || "$1" == "latest" ]]; then
                sandbox_dir=$(get_latest_sandbox)
                [[ -z "$sandbox_dir" ]] && { echo "Error: No sandbox found." >&2; exit 1; }
                [[ -n "$1" ]] && shift
            else
                sandbox_dir="$SANDBOXES_DIR/$1"; shift
            fi

            local target_dir=""
            if [[ -z "$1" || "$1" == "latest" ]]; then
                target_dir=$(get_latest_target)
                [[ -z "$target_dir" ]] && { echo "Error: No target found." >&2; exit 1; }
                [[ -n "$1" ]] && shift
            else
                target_dir="$TARGETS_DIR/$1"; shift
            fi

            local target_arch="${1:-riscv64}"; shift

            [[ ! -d "$sandbox_dir" ]] && { echo "Error: Sandbox not found: $sandbox_dir" >&2; exit 1; }
            [[ ! -d "$target_dir" ]] && { echo "Error: Target not found: $target_dir" >&2; exit 1; }

            install_packages "$sandbox_dir" "$target_dir" "$target_arch" "$@"
            ;;
        install-from)
            local sandbox_dir=""
            if [[ -z "$1" || "$1" == "latest" ]]; then
                sandbox_dir=$(get_latest_sandbox)
                [[ -z "$sandbox_dir" ]] && { echo "Error: No sandbox found." >&2; exit 1; }
                [[ -n "$1" ]] && shift
            else
                sandbox_dir="$SANDBOXES_DIR/$1"; shift
            fi

            local target_dir=""
            if [[ -z "$1" || "$1" == "latest" ]]; then
                target_dir=$(get_latest_target)
                [[ -z "$target_dir" ]] && { echo "Error: No target found." >&2; exit 1; }
                [[ -n "$1" ]] && shift
            else
                target_dir="$TARGETS_DIR/$1"; shift
            fi

            local target_arch="${1:-riscv64}"; shift
            local pkg_file="$1"

            [[ ! -d "$sandbox_dir" ]] && { echo "Error: Sandbox not found: $sandbox_dir" >&2; exit 1; }
            [[ ! -d "$target_dir" ]] && { echo "Error: Target not found: $target_dir" >&2; exit 1; }
            [[ ! -f "$pkg_file" ]] && { echo "Error: Package file not found: $pkg_file" >&2; exit 1; }

            # shellcheck disable=SC2046
            install_packages "$sandbox_dir" "$target_dir" "$target_arch" \
                $(packages_from_file "$pkg_file")
            ;;
        update-ldconfig)
            local sandbox_dir=""
            if [[ -z "$1" || "$1" == "latest" ]]; then
                sandbox_dir=$(get_latest_sandbox)
                [[ -z "$sandbox_dir" ]] && { echo "Error: No sandbox found." >&2; exit 1; }
                [[ -n "$1" ]] && shift
            else
                sandbox_dir="$SANDBOXES_DIR/$1"; shift
            fi

            local target_dir=""
            if [[ -z "$1" || "$1" == "latest" ]]; then
                target_dir=$(get_latest_target)
                [[ -z "$target_dir" ]] && { echo "Error: No target found." >&2; exit 1; }
                [[ -n "$1" ]] && shift
            else
                target_dir="$TARGETS_DIR/$1"; shift
            fi

            [[ ! -d "$sandbox_dir" ]] && { echo "Error: Sandbox not found: $sandbox_dir" >&2; exit 1; }
            [[ ! -d "$target_dir" ]] && { echo "Error: Target not found: $target_dir" >&2; exit 1; }

            update_ldconfig_sandbox "$sandbox_dir" "$target_dir"
            ;;
        *)
            usage
            ;;
    esac
}

main "$@"
