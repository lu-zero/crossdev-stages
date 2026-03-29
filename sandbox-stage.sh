#! /bin/bash
# More structured sandbox prototype
#
# - cache the stage3 in a .cache path
# - unpack either in a known place in $CACHE_DIR/sandboxes/ or as needed
# - provide an enter/run command that by default uses the latest sandbox

CACHE_DIR="${HOME}/.cache/crossdev-stages"
STAGES_DIR="${CACHE_DIR}/stages"
SANDBOXES_DIR="${CACHE_DIR}/sandboxes"

ensure_cache_dirs() {
    mkdir -p "$STAGES_DIR"
    mkdir -p "$SANDBOXES_DIR"
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

    # Add rust-std workaround
    echo "cross-${ARCH}-unknown-linux-gnu/rust-std **" > "$sandbox_dir/etc/portage/package.accept_keywords/rust-std"

    echo "Portage configured for ${ARCH} in $sandbox_dir"
}

install_dependencies() {
    local sandbox_dir="$1"

    echo "Installing host system dependencies in sandbox..."

    # Host system dependencies from README.md (with categories)
    local packages=(
        "app-arch/zstd"
        "sys-devel/crossdev"
        "sys-apps/merge-usr"
        "dev-vcs/git"
        "sys-boot/u-boot-tools"
        "sys-apps/dtc"
        "sys-kernel/dracut"
        "sys-apps/busybox"
        "sys-boot/genimage"
        "app-arch/xz-utils"
        "app-eselect/eselect-repository"
    )

    # Update package database first
    echo "Running getuto..."
    run "$sandbox_dir" getuto || echo "getuto failed, continuing anyway..."

    # Use the run command to emerge all packages at once with -G flag
    echo "Emerging all host dependencies..."
    run "$sandbox_dir" emerge -G "${packages[@]}" || echo "Some packages failed to emerge"

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
      -- $args
}

usage() {
    echo "$0 <setup|prepare|enter|run> [options]"
    echo "$0 setup [arch] [name]   - Setup sandbox for arch (default: host arch, name: arch)"
    echo "$0 prepare [sandbox]     - Prepare sandbox with Portage config and host dependencies"
    echo "$0 enter [sandbox]      - Enter interactive shell in sandbox (default: latest)"
    echo "$0 run <sandbox> <cmd>  - Run command in specified sandbox"
    echo ""
    echo "Sandboxes are cached in: $CACHE_DIR"
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
            shift
            local sandbox_dir=""
            if [[ -n "$1" ]]; then
                sandbox_dir="$SANDBOXES_DIR/$1"
                shift
            else
                sandbox_dir=$(get_latest_sandbox)
            fi

            if [[ ! -d "$sandbox_dir" ]]; then
                echo "Error: No sandbox found. Please run setup first." >&2
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
        enter)
            local sandbox_dir=""
            if [[ -n "$1" ]]; then
                sandbox_dir="$SANDBOXES_DIR/$1"
            else
                sandbox_dir=$(get_latest_sandbox)
            fi

            if [[ ! -d "$sandbox_dir" ]]; then
                echo "Error: No sandbox found. Please run setup first." >&2
                exit 1
            fi

            run "$sandbox_dir" bash --login
            ;;
        run)
            local sandbox_dir="$SANDBOXES_DIR/$1"
            shift

            if [[ ! -d "$sandbox_dir" ]]; then
                echo "Error: Sandbox not found: $sandbox_dir" >&2
                exit 1
            fi

            run "$sandbox_dir" "$@"
            ;;
        *)
            usage
            ;;
    esac
}

main "$@"
