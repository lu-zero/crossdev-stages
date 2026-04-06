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
    echo "$0 <setup|enter|run> [options]"
    echo "$0 setup [arch] [name]   - Setup sandbox for arch (default: host arch, name: arch)"
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
