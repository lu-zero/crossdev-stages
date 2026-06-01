# Common functions for crossdev-stages
# Source this from other scripts: source "$BASE_DIR/lib/common.sh"

CACHE_DIR="${HOME}/.cache/crossdev-stages"
STAGES_DIR="${CACHE_DIR}/stages"
SANDBOXES_DIR="${CACHE_DIR}/sandboxes"
TARGETS_DIR="${CACHE_DIR}/targets"
BUILDS_DIR="${CACHE_DIR}/builds"
SYSROOTS_DIR="${CACHE_DIR}/sysroots"
LDCONFIG="/usr/local/bin/ldconfig"

ensure_cache_dirs() {
    mkdir -p "$STAGES_DIR"
    mkdir -p "$SANDBOXES_DIR"
    mkdir -p "$TARGETS_DIR"
    mkdir -p "$BUILDS_DIR"
    mkdir -p "$SYSROOTS_DIR"
}

# Helper function to set or replace a variable in make.conf
set_make_conf_var() {
    local file="$1"
    local var_name="$2"
    local var_value="$3"

    if grep -q "^${var_name}=" "$file"; then
        local tmpf="${file}.tmp"
        awk -v name="$var_name" -v val="$var_value" \
            '$0 ~ "^"name"=" { print name"=\""val"\""; next } { print }' \
            "$file" > "$tmpf" && mv -f "$tmpf" "$file"
    else
        echo "${var_name}=\"${var_value}\"" >> "$file"
    fi
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

get_build_board() {
    local dir="$1"
    if [[ -f "$dir/.board" ]]; then
        cat "$dir/.board"
    else
        echo ""
    fi
}

get_latest_build() {
    if [[ -d "$BUILDS_DIR" ]]; then
        local latest
        latest=$(ls -t "$BUILDS_DIR" | head -n 1)
        if [[ -n "$latest" ]]; then
            echo "$BUILDS_DIR/$latest"
            return 0
        fi
    fi
    echo "" >&2
    return 1
}

# We use hakoniwa even here to preserve the owners, same for removal
remove_dir() {
    local dir="$1"
    local parent
    parent=$(dirname "$dir")
    local name
    name=$(basename "$dir")

    hakoniwa run \
      --rootfs / --devfs /dev \
      --allow-new-privs \
      --userns=auto \
      --tmpfs /dev/shm \
      --tmpfs /tmp \
      -B "$parent":/target \
      -- /bin/sh -c "rm -rf \"/target/$name\""
}

# --- hakoniwa run wrappers ---
#
# If CURRENT_SYSROOT is set (host path to a sysroot directory),
# all run functions automatically bind-mount it as /usr/$CURRENT_CHOST.
# Set these before calling run functions to enable sysroot isolation.
CURRENT_SYSROOT=""
CURRENT_CHOST=""

# Internal: run hakoniwa with common flags + extra bind mounts
# Usage: _hakoniwa_run <sandbox_dir> <script> [extra hakoniwa args...]
_hakoniwa_run() {
    local sandbox_dir="$1"
    local script="$2"
    shift 2

    local sysroot_args=()
    if [[ -n "$CURRENT_SYSROOT" && -n "$CURRENT_CHOST" ]]; then
        sysroot_args+=(-B "$CURRENT_SYSROOT":/usr/"$CURRENT_CHOST")
    fi

    hakoniwa run \
      --rootdir "$sandbox_dir":rw \
      --devfs /dev \
      -b /etc/resolv.conf \
      --unshare-all \
      --allow-new-privs \
      --userns=auto \
      --network=host \
      --tmpfs /tmp \
      --tmpfs /dev/shm \
      ${sysroot_args[@]+"${sysroot_args[@]}"} \
      "$@" \
      -e TERM="$TERM" \
      -e COLORTERM="${COLORTERM:-}" \
      -e NO_COLOR="${NO_COLOR:-}" \
      -e HOME=/root \
      -e CONFIG_CHECK="" \
      -- bash --login -c "$script"
}

run() {
    local sandbox_dir="$1"
    shift
    _hakoniwa_run "$sandbox_dir" "$*"
}

run_with_stage() {
    local sandbox_dir="$1"
    local stage_dir="$2"
    shift 2
    _hakoniwa_run "$sandbox_dir" "$*" \
      -B "$stage_dir":/target
}

run_with_build() {
    local sandbox_dir="$1"
    local build_dir="$2"
    shift 2
    _hakoniwa_run "$sandbox_dir" "$*" \
      -B "$build_dir":/build \
      -b "$BASE_DIR":/scripts
}

run_with_build_and_source() {
    local sandbox_dir="$1"
    local build_dir="$2"
    local source_dir="$3"
    shift 3
    # collect extra hakoniwa args until "--" sentinel
    local extra_args=()
    while [[ $# -gt 0 && "$1" != "--" ]]; do
        extra_args+=("$1"); shift
    done
    [[ $# -gt 0 && "$1" == "--" ]] && shift
    _hakoniwa_run "$sandbox_dir" "$*" \
      -B "$build_dir":/build \
      -b "$source_dir":/target_src \
      -b "$BASE_DIR":/scripts \
      ${extra_args[@]+"${extra_args[@]}"}
}

# resolve_dir <nameref> <base_dir> <get_latest_fn> <label> [arg]
# Sets <nameref> to base_dir/arg, or to the latest entry if arg is absent or "latest".
resolve_dir() {
    local -n _rd_out=$1
    local base="$2"
    local get_latest="$3"
    local label="$4"
    local arg="${5-}"

    if [[ -z "$arg" || "$arg" == "latest" ]]; then
        _rd_out=$($get_latest) || true
        if [[ -z "$_rd_out" || ! -d "$_rd_out" ]]; then
            echo "Error: No $label found." >&2
            exit 1
        fi
    else
        _rd_out="$base/$arg"
    fi
}

resolve_build()  { resolve_dir "$1" "$BUILDS_DIR"  get_latest_build  "build"  "${2-}"; }
resolve_target() { resolve_dir "$1" "$TARGETS_DIR" get_latest_target "target" "${2-}"; }

packages_from_file() {
    local file="$1"
    grep -v '#' "$file" | grep -v '^[[:space:]]*$' || true
}

# Map OS architecture to Gentoo variables
gentoo_arch() {
    local os_arch=$1
    case $os_arch in
        x86_64)   ARCH=amd64 FLAVOR=amd64-openrc;;
        aarch64)  ARCH=arm64 FLAVOR=arm64-openrc;;
        riscv*)   ARCH=riscv FLAVOR=rv64_lp64d-openrc;;
        i[3456]86) ARCH=x86 FLAVOR=x86-openrc;;
        *) echo "Error: Unknown architecture: $os_arch" >&2; return 1;;
    esac
}

# Board-specific BOARD_CFLAGS (from board.conf) takes precedence
target_cflags() {
    local arch=$1
    if [[ -n "${BOARD_CFLAGS:-}" ]]; then
        echo "$BOARD_CFLAGS"
        return
    fi
    case $arch in
        x86_64)    echo "-O3 -march=x86-64 -pipe" ;;
        aarch64)   echo "-O3 -pipe" ;;
        riscv64)   echo "-O3 -march=rv64gc -pipe" ;;
        i[3456]86) echo "-O2 -march=${arch} -pipe" ;;
        *)         echo "-O3 -pipe" ;;
    esac
}

# Map OS architecture to Gentoo profile path
gentoo_profile() {
    local arch=$1
    gentoo_arch "$arch"
    case "$ARCH" in
        riscv) echo "default/linux/riscv/23.0/rv64/lp64d" ;;
        x86)   echo "default/linux/x86/23.0" ;;
        *)     echo "default/linux/${ARCH}/23.0" ;;
    esac
}

llvm_arch() {
    local arch=$1
    local llvm_target=""
    case $arch in
        x86_64)    llvm_target="X86" ;;
        i[3456]86) llvm_target="X86" ;;
        aarch64)   llvm_target="AArch64" ;;
        riscv*)    llvm_target="RISCV" ;;
        *)         ;;
    esac
    if [[ -n "$llvm_target" ]]; then
        echo "$llvm_target"
    fi
}
