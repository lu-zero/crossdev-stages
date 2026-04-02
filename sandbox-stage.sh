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
LDCONFIG="/usr/local/bin/ldconfig"

ensure_cache_dirs() {
    mkdir -p "$STAGES_DIR"
    mkdir -p "$SANDBOXES_DIR"
    mkdir -p "$TARGETS_DIR"
}

get_arch() {
    local dir="$1"
    if [[ -f "$dir/.arch" ]]; then
        cat "$dir/.arch"
    else
        echo ""
    fi
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

    local profile
    profile=$(gentoo_profile "$target_arch")

    local crossdev_root="/usr/${chost}"
    local crossdev_make_conf="${crossdev_root}/etc/portage/make.conf"
    local cflags
    cflags=$(target_cflags "$target_arch")

    echo "Setting up crossdev environment for ${chost} in sandbox..."

    # Create the crossdev overlay
    run "$sandbox_dir" eselect repository create crossdev

    # Initialize crossdev for target architecture
    run "$sandbox_dir" crossdev "${chost}" --init-target

    # Add rust-std workaround
    run "$sandbox_dir" "echo \"cross-${target_arch}-unknown-linux-gnu/rust-std **\" > /etc/portage/package.accept_keywords/rust-std"

    # Set up portage profile (crossdev links a wrong default)
    run "$sandbox_dir" "export PORTAGE_CONFIGROOT=${crossdev_root}; eselect profile set ${profile}"

    # Configure make.conf for cross environment
    local host_make_conf="$sandbox_dir${crossdev_make_conf}"
    local cpu_count=$(nproc 2>/dev/null || echo 4)
    local p=$((cpu_count / 2 + 1))
    local q="$cpu_count"
    set_make_conf_var "$host_make_conf" "CFLAGS" "${cflags}"
    set_make_conf_var "$host_make_conf" "CXXFLAGS" "${cflags}"
    set_make_conf_var "$host_make_conf" "MAKEOPTS" "-j${p} --load-average ${q}"
    set_make_conf_var "$host_make_conf" "EMERGE_DEFAULT_OPTS" "--jobs=${p} --load-average ${q}"
    set_make_conf_var "$host_make_conf" "FEATURES" "parallel-install -merge-wait"

    local llvm_target=$(llvm_arch "$target_arch")
    if [[ -n "$llvm_target" ]]; then
        set_make_conf_var "$host_make_conf" "LLVM_TARGETS" "${llvm_target}"
    fi

    # Write crossdev portage config directly on the host filesystem
    local host_crossdev_root="$sandbox_dir${crossdev_root}"
    mkdir -p "$host_crossdev_root/etc/portage/env"
    mkdir -p "$host_crossdev_root/etc/portage/package.env"
    mkdir -p "$host_crossdev_root/etc/portage/package.use"
    mkdir -p "$host_crossdev_root/etc/portage/package.accept_keywords"

    # env/plain.conf: strip arch-specific flags (used for packages like rust)
    cat > "$host_crossdev_root/etc/portage/env/plain.conf" << 'EOF'
CFLAGS="-O3 -pipe"
CXXFLAGS="-O3 -pipe"
EOF

    # package.env
    echo "dev-lang/rust plain.conf" > "$host_crossdev_root/etc/portage/package.env/rust"

    # package.use
    cat > "$host_crossdev_root/etc/portage/package.use/busybox" << 'EOF'
>=virtual/libcrypt-2-r1 static-libs
>=sys-libs/libxcrypt-4.4.36-r3 static-libs
>=sys-apps/busybox-1.36.1-r3 -pam static
EOF
    echo "llvm-core/clang -extra" > "$host_crossdev_root/etc/portage/package.use/clang"
    echo "dev-lang/rust rustfmt -system-llvm" > "$host_crossdev_root/etc/portage/package.use/rust"
    echo "dev-vcs/git -iconv" > "$host_crossdev_root/etc/portage/package.use/git"

    # package.accept_keywords
    echo "<sys-devel/gcc-16.0.9999:16 **" > "$host_crossdev_root/etc/portage/package.accept_keywords/gcc"

    # Apply host sandbox workarounds
    run "$sandbox_dir" mkdir -p "/etc/portage/package.{accept_keywords,mask}"

    # Fix split-usr layout created by crossdev before emerging into the sysroot
    run "$sandbox_dir" mkdir "${crossdev_root}/bin"
    run "$sandbox_dir" merge-usr --root "${crossdev_root}"

    # Install crossdev toolchain
    run "$sandbox_dir" crossdev "${chost}" --ex-pkg sys-devel/clang-crossdev-wrappers --ex-pkg sys-devel/rust-std

    touch "$sandbox_dir/.crossdev-${target_arch}"
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
        "sys-kernel/gentoo-sources"
    )

    echo "Syncing portage tree..."
    run "$sandbox_dir" emerge-webrsync

    echo "Running getuto..."
    run "$sandbox_dir" getuto || echo "getuto failed, continuing anyway..."

    echo "Emerging bin dependencies..."
    run "$sandbox_dir" emerge -G "${bin_packages[@]}" || echo "Some packages failed to emerge"
    echo "Emerging dependencies..."
    run "$sandbox_dir" emerge -b -k "${packages[@]}" || echo "Some packages failed to emerge"

    echo "Installing Rust ldconfig..."
    run "$sandbox_dir" cargo install --root /usr/local ldconfig
    # Installed to $LDCONFIG inside the sandbox

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

    touch "$sandbox_dir/.prepared"
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

# Map OS architecture to default CFLAGS for cross-compilation
target_cflags() {
    local arch=$1
    case $arch in
        x86_64)  echo "-O3 -march=x86-64 -pipe" ;;
        aarch64) echo "-O3 -pipe" ;;
        riscv64) echo "-O3 -march=rv64gc -pipe" ;;
        *)       echo "-O3 -pipe" ;;
    esac
}

# Map OS architecture to Gentoo profile path
gentoo_profile() {
    local arch=$1
    gentoo_arch "$arch"
    case "$ARCH" in
        riscv) echo "default/linux/riscv/23.0/rv64/lp64d" ;;
        *)     echo "default/linux/${ARCH}/23.0" ;;
    esac
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

verify_stage() {
    local stage_file="$1"
    local digest_file="$2"
    local filename=$(basename "$stage_file")

    local expected
    expected=$(awk '/# SHA512 HASH/{found=1; next} found && /'"$filename"'$/ && !/CONTENTS/{print $1; exit}' "$digest_file")

    if [[ -z "$expected" ]]; then
        echo "Warning: SHA512 hash not found in digests for $filename" >&2
        return 0
    fi

    local actual
    actual=$(sha512sum "$stage_file" | awk '{print $1}')

    if [[ "$actual" == "$expected" ]]; then
        echo "Hash verified: $filename" >&2
        return 0
    else
        echo "Hash mismatch: $filename (corrupt or incomplete download)" >&2
        return 1
    fi
}

fetch_stage() {
    local arch=$1
    gentoo_arch $arch
    STAGE="stage3-$FLAVOR"
    BASE_URL="https://distfiles.gentoo.org/releases/$ARCH/autobuilds/"
    LATEST_URL="$BASE_URL/latest-$STAGE.txt"
    STAGE3_FILE=$(curl $LATEST_URL -s -f | grep -B1 'BEGIN PGP SIGNATURE' | head -n 1 | cut -d\  -f 1)
    STAGE3_URL="$BASE_URL/$STAGE3_FILE"

    echo "Fetching $STAGE3_FILE" >&2

    ensure_cache_dirs

    local stage_filename=$(basename "$STAGE3_FILE")
    local stage_file_path="$STAGES_DIR/$stage_filename"
    local digest_file_path="$stage_file_path.DIGESTS"

    curl -sLf "$STAGE3_URL.DIGESTS" -o "$digest_file_path" || {
        echo "Failed to download digests for $STAGE3_FILE" >&2
        return 1
    }

    if [[ -f "$stage_file_path" ]]; then
        echo "$stage_filename already cached, verifying..." >&2
        if ! verify_stage "$stage_file_path" "$digest_file_path"; then
            echo "Removing corrupt cached file, re-downloading..." >&2
            rm -f "$stage_file_path"
        fi
    fi

    if [[ ! -f "$stage_file_path" ]]; then
        curl -Lf "$STAGE3_URL" -o "$stage_file_path" || {
            echo "Failed to download $STAGE3_FILE" >&2
            rm -f "$stage_file_path"
            return 1
        }
        verify_stage "$stage_file_path" "$digest_file_path" || {
            rm -f "$stage_file_path"
            return 1
        }
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
        echo "Sandbox $sandbox_name already exists" >&2
        echo "$sandbox_dir"
        return 0
    fi

    echo "Creating sandbox $sandbox_name from $stage_file" >&2

    hakoniwa run \
      --rootfs / --devfs /dev \
      --unshare-all \
      --allow-new-privs \
      --userns=auto \
      --tmpfs /tmp \
      -B "$CACHE_DIR":/cache \
      -- /bin/sh -c "
        mkdir -p \"/cache/sandboxes/$sandbox_name\" &&
        tar --overwrite -xpf \"/cache/stages/$stage_filename\" \
          --xattrs-include='*.*' \
          --numeric-owner \
          --exclude='./dev' \
          -C \"/cache/sandboxes/$sandbox_name\" &&
        echo \"/cache/sandboxes/$sandbox_name\"
      "
}

run() {
    local sandbox_dir="$1"
    shift
    local args="$*"

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
    local args="$*"

    hakoniwa run \
      --rootdir "$sandbox_dir":rw \
      --devfs /dev \
      -b /etc/resolv.conf \
      --unshare-all \
      --allow-new-privs \
      --userns=auto \
      --network=host \
      --tmpfs /tmp \
      -B "$stage_dir":/target \
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
        echo "Target $target_name already exists" >&2
        echo "$target_dir"
        return 0
    fi

    echo "Creating target $target_name from $stage_file" >&2

    hakoniwa run \
      --rootfs / --devfs /dev \
      --unshare-all \
      --allow-new-privs \
      --userns=auto \
      --tmpfs /tmp \
      -B "$CACHE_DIR":/cache \
      -- /bin/sh -c "
        mkdir -p \"/cache/targets/$target_name\" &&
        tar --overwrite -xpf \"/cache/stages/$stage_filename\" \
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
    run_with_stage "$sandbox_dir" "$stage_dir" "$LDCONFIG -v -r /target"
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
    run_with_stage "$sandbox_dir" "$stage_dir" "KERNEL_DIR=/usr/src/linux ROOT=/target ${chost}-emerge -b -k -e @world"
    update_ldconfig_sandbox "$sandbox_dir" "$stage_dir"
    echo "$(date -u +%Y%m%dT%H%M%SZ)" >> "$stage_dir/.updated"
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
    echo "$(date -u +%Y%m%dT%H%M%SZ) $*" >> "$stage_dir/.packages"
}

packages_from_file() {
    local file="$1"
    grep -v '#' "$file" | grep -v '^[[:space:]]*$'
}

ensure_crossdev() {
    local sandbox_dir="$1"
    local target_arch="${2:-}"

    # Auto-setup sandbox if none exists
    if [[ ! -d "$sandbox_dir" ]]; then
        local host_arch=$(uname -m)
        echo "No sandbox found, setting up sandbox for host arch ($host_arch)..."
        local stage_file
        stage_file=$(fetch_stage "$host_arch") || return 1
        unpack_stage "$stage_file" "$host_arch" || return 1
        sandbox_dir="$SANDBOXES_DIR/$host_arch"
        echo "$host_arch" > "$sandbox_dir/.arch"
    fi

    local arch
    arch=$(get_arch "$sandbox_dir")
    if [[ -z "$arch" ]]; then
        echo "Error: No .arch metadata in $sandbox_dir (old sandbox?)" >&2
        return 1
    fi

    # Auto-prepare sandbox if not yet prepared
    if [[ ! -f "$sandbox_dir/.prepared" ]]; then
        echo "Sandbox not prepared, running prepare..."
        prepare_sandbox "$sandbox_dir" "$arch"
    fi

    # Default target arch to sandbox host arch if not specified
    : "${target_arch:=$arch}"

    # Auto-setup crossdev if not yet done for this target arch
    if [[ ! -f "$sandbox_dir/.crossdev-${target_arch}" ]]; then
        setup_crossdev_sandbox "$sandbox_dir" "$target_arch"
    fi
}

ensure_target() {
    local arch="$1"
    local target_name="$2"
    local target_dir="$TARGETS_DIR/$target_name"

    if [[ ! -d "$target_dir" ]]; then
        local stage_file
        stage_file=$(fetch_stage "$arch") || return 1
        unpack_target "$stage_file" "$target_name"
        echo "$arch" > "$target_dir/.arch"
    fi

    ensure_crossdev "$(resolve_sandbox)" "$arch" || return 1
}

usage() {
    echo "$0 [-s|--sandbox <name>] <command> [options]"
    echo ""
    echo "Global options:"
    echo "  -s, --sandbox <name>           - Use named sandbox (default: latest)"
    echo ""
    echo "Sandbox commands:"
    echo "  $0 setup [arch] [name]         - Setup sandbox for arch (default: host arch, name: arch)"
    echo "  $0 list                        - List sandboxes"
    echo "  $0 destroy [name]              - Remove a sandbox (name or -s)"
    echo "  $0 prepare [--manual]          - Prepare sandbox (--manual: configure then enter shell)"
    echo "  $0 setup-crossdev [target-arch] - Setup cross-compilation environment in sandbox"
    echo "  $0 enter                       - Enter interactive shell in sandbox"
    echo "  $0 run <cmd>                   - Run command in sandbox"
    echo ""
    echo "Target commands:"
    echo "  $0 target list                 - List unpacked targets"
    echo "  $0 target setup [arch] [name]  - Setup target sysroot for arch"
    echo "  $0 target destroy <name>       - Remove a target"
    echo "  $0 target update [target] [arch] - Update target via cross-emerge"
    echo "  $0 target pack [target] [arch] - Pack target as stage3 tarball in stages cache"
    echo ""
    echo "Package install commands:"
    echo "  $0 install [target] [arch] pkg... - Install packages into target"
    echo "  $0 install-from [target] [arch] file - Install packages from file"
    echo "  $0 update-ldconfig [target]    - Regenerate ld.so.cache in target"
    echo ""
    echo "Cache directory: $CACHE_DIR"
    exit 1
}

main() {
    # Parse global --sandbox / -s flag before the command
    local opt_sandbox=""
    local filtered_args=()
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --sandbox|-s) opt_sandbox="$2"; shift 2 ;;
            *) filtered_args+=("$1"); shift ;;
        esac
    done
    set -- "${filtered_args[@]}"

    # Resolve sandbox dir: from flag, or latest
    resolve_sandbox() {
        if [[ -n "$opt_sandbox" ]]; then
            echo "$SANDBOXES_DIR/$opt_sandbox"
        else
            get_latest_sandbox
        fi
    }

    local cmd="$1"
    shift

    case $cmd in
        setup)
            local arch="${1:-$(uname -m)}"
            [[ -n "$1" ]] && shift
            local sandbox_name="${1:-$arch}"
            [[ -n "$1" ]] && shift

            echo "Setting up sandbox: $sandbox_name"
            local stage_file
            stage_file=$(fetch_stage "$arch") || exit 1
            local sandbox_dir
            sandbox_dir=$(unpack_stage "$stage_file" "$sandbox_name") || exit 1
            echo "$arch" > "$SANDBOXES_DIR/$sandbox_name/.arch"
            echo "Sandbox ready: $sandbox_dir"
            ;;
        list)
            if [[ -d "$SANDBOXES_DIR" ]]; then
                for d in "$SANDBOXES_DIR"/*/; do
                    [[ -d "$d" ]] || continue
                    local name=$(basename "$d")
                    local arch=$(get_arch "$d")
                    local state="setup"
                    [[ -f "$d/.prepared" ]] && state="prepared"
                    local crossdev_targets=()
                    for f in "$d"/.crossdev-*; do
                        [[ -f "$f" ]] && crossdev_targets+=("${f##*/.crossdev-}")
                    done
                    if [[ ${#crossdev_targets[@]} -gt 0 ]]; then
                        state="crossdev(${crossdev_targets[*]})"
                    fi
                    printf "%-20s %-10s %s\n" "$name" "${arch:-(unknown)}" "$state"
                done
            else
                echo "No sandboxes found."
            fi
            ;;
        destroy)
            if [[ -z "$1" && -z "$opt_sandbox" ]]; then
                echo "Usage: $0 destroy <sandbox-name>" >&2
                exit 1
            fi
            local name="${1:-$opt_sandbox}"
            local target="$SANDBOXES_DIR/$name"
            if [[ ! -d "$target" ]]; then
                echo "Error: Sandbox not found: $name" >&2
                exit 1
            fi
            echo "Removing sandbox: $name"
            rm -rf "$target"
            echo "Sandbox $name removed."
            ;;
        prepare)
            local manual=0
            [[ "$1" == "--manual" ]] && { manual=1; shift; }

            local sandbox_dir
            sandbox_dir=$(resolve_sandbox)
            if [[ -z "$sandbox_dir" || ! -d "$sandbox_dir" ]]; then
                echo "Error: No sandbox found. Please run setup first." >&2
                exit 1
            fi

            local arch
            arch=$(get_arch "$sandbox_dir")
            if [[ -z "$arch" ]]; then
                echo "Error: No .arch metadata in $sandbox_dir (old sandbox?)" >&2
                exit 1
            fi

            if [[ $manual -eq 1 ]]; then
                configure_portage "$sandbox_dir" "$arch"
                echo "Portage configured. Entering sandbox for manual setup..."
                run "$sandbox_dir" bash --login
            else
                prepare_sandbox "$sandbox_dir" "$arch"
            fi
            ;;
        setup-crossdev)
            local target_arch="${1:-}"
            [[ -n "$1" ]] && shift

            local sandbox_dir
            sandbox_dir=$(resolve_sandbox)
            ensure_crossdev "$sandbox_dir" "${target_arch:-}"
            ;;
        enter)
            local sandbox_dir
            sandbox_dir=$(resolve_sandbox)
            if [[ -z "$sandbox_dir" || ! -d "$sandbox_dir" ]]; then
                echo "Error: No sandbox found. Please run setup first." >&2
                exit 1
            fi
            run "$sandbox_dir" bash --login
            ;;
        run)
            local sandbox_dir
            sandbox_dir=$(resolve_sandbox)
            if [[ -z "$sandbox_dir" || ! -d "$sandbox_dir" ]]; then
                echo "Error: No sandbox found. Please run setup first." >&2
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
                        for d in "$TARGETS_DIR"/*/; do
                            [[ -d "$d" ]] || continue
                            local name=$(basename "$d")
                            local arch=$(get_arch "$d")
                            local state="setup"
                            if [[ -f "$d/.updated" ]]; then
                                local last_update
                                last_update=$(tail -n1 "$d/.updated")
                                state="updated($last_update)"
                            fi
                            if [[ -f "$d/.packages" ]]; then
                                local pkg_count
                                pkg_count=$(wc -l < "$d/.packages")
                                state="${state} packages(${pkg_count})"
                            fi
                            printf "%-20s %-10s %s\n" "$name" "${arch:-(unknown)}" "${state# }"
                        done
                    else
                        echo "No targets found."
                    fi
                    ;;
                destroy)
                    if [[ -z "$1" ]]; then
                        echo "Usage: $0 target destroy <target-name>" >&2
                        exit 1
                    fi
                    local target="$TARGETS_DIR/$1"
                    if [[ ! -d "$target" ]]; then
                        echo "Error: Target not found: $1" >&2
                        exit 1
                    fi
                    echo "Removing target: $1"
                    rm -rf "$target"
                    echo "Target $1 removed."
                    ;;
                setup)
                    local arch="${1:-$(uname -m)}"
                    [[ -n "$1" ]] && shift
                    local target_name="${1:-$arch}"
                    [[ -n "$1" ]] && shift

                    ensure_target "$arch" "$target_name" || exit 1
                    ;;
                update)
                    local target_dir=""
                    if [[ -z "$1" || "$1" == "latest" ]]; then
                        target_dir=$(get_latest_target)
                        [[ -z "$target_dir" ]] && { echo "Error: No target found." >&2; exit 1; }
                        [[ -n "$1" ]] && shift
                    else
                        target_dir="$TARGETS_DIR/$1"; shift
                    fi

                    local target_arch="${1:-$(get_arch "$target_dir")}"
                    [[ -n "$1" ]] && shift
                    [[ -z "$target_arch" ]] && { echo "Error: Cannot determine target arch. Specify explicitly." >&2; exit 1; }

                    local target_name=$(basename "$target_dir")
                    ensure_target "$target_arch" "$target_name" || exit 1

                    local sandbox_dir
                    sandbox_dir=$(resolve_sandbox)
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

                    local target_arch="${1:-$(get_arch "$target_dir")}"
                    [[ -n "$1" ]] && shift
                    [[ -z "$target_arch" ]] && { echo "Error: Cannot determine target arch. Specify explicitly." >&2; exit 1; }
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
                      -b "$target_dir":/target \
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
            local target_dir=""
            if [[ -z "$1" || "$1" == "latest" ]]; then
                target_dir=$(get_latest_target)
                [[ -z "$target_dir" ]] && { echo "Error: No target found." >&2; exit 1; }
                [[ -n "$1" ]] && shift
            else
                target_dir="$TARGETS_DIR/$1"; shift
            fi

            local target_arch="${1:-$(get_arch "$target_dir")}"; [[ -n "$1" ]] && shift
            [[ -z "$target_arch" ]] && { echo "Error: Cannot determine target arch. Specify explicitly." >&2; exit 1; }

            local sandbox_dir
            sandbox_dir=$(resolve_sandbox)
            [[ -z "$sandbox_dir" ]] && { echo "Error: No sandbox found." >&2; exit 1; }

            [[ ! -d "$sandbox_dir" ]] && { echo "Error: Sandbox not found: $sandbox_dir" >&2; exit 1; }
            [[ ! -d "$target_dir" ]] && { echo "Error: Target not found: $target_dir" >&2; exit 1; }

            install_packages "$sandbox_dir" "$target_dir" "$target_arch" "$@"
            ;;
        install-from)
            local target_dir=""
            if [[ -z "$1" || "$1" == "latest" ]]; then
                target_dir=$(get_latest_target)
                [[ -z "$target_dir" ]] && { echo "Error: No target found." >&2; exit 1; }
                [[ -n "$1" ]] && shift
            else
                target_dir="$TARGETS_DIR/$1"; shift
            fi

            local target_arch="${1:-$(get_arch "$target_dir")}"; [[ -n "$1" ]] && shift
            [[ -z "$target_arch" ]] && { echo "Error: Cannot determine target arch. Specify explicitly." >&2; exit 1; }

            local sandbox_dir
            sandbox_dir=$(resolve_sandbox)
            [[ -z "$sandbox_dir" ]] && { echo "Error: No sandbox found." >&2; exit 1; }

            local pkg_file="$1"

            [[ ! -d "$sandbox_dir" ]] && { echo "Error: Sandbox not found: $sandbox_dir" >&2; exit 1; }
            [[ ! -d "$target_dir" ]] && { echo "Error: Target not found: $target_dir" >&2; exit 1; }
            [[ ! -f "$pkg_file" ]] && { echo "Error: Package file not found: $pkg_file" >&2; exit 1; }

            # shellcheck disable=SC2046
            install_packages "$sandbox_dir" "$target_dir" "$target_arch" \
                $(packages_from_file "$pkg_file")
            ;;
        update-ldconfig)
            local target_dir=""
            if [[ -z "$1" || "$1" == "latest" ]]; then
                target_dir=$(get_latest_target)
                [[ -z "$target_dir" ]] && { echo "Error: No target found." >&2; exit 1; }
                [[ -n "$1" ]] && shift
            else
                target_dir="$TARGETS_DIR/$1"; shift
            fi

            local sandbox_dir
            sandbox_dir=$(resolve_sandbox)
            [[ -z "$sandbox_dir" ]] && { echo "Error: No sandbox found." >&2; exit 1; }

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
