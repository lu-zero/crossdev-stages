# Board configuration loader
# Source this from other scripts: source "$BASE_DIR/lib/board.sh"
#
# Requires: BASE_DIR set before sourcing
#
# After load_board_config():
#   BOARD_NAME, BOARD_ARCH, BOARD_CFLAGS, SYSROOT
#   BOARD_CFG_DIR (directory containing the board config)
#   board_*() functions from optional board.sh

load_board_config() {
    local board="$1"
    local cfg="$BASE_DIR/boards/$board/board.conf"
    [[ -f "$cfg" ]] || { echo "Board config not found: $cfg" >&2; return 1; }
    # shellcheck source=/dev/null
    source "$cfg"
    BOARD_CFG_DIR="$BASE_DIR/boards/$board"

    # SYSROOT is required
    if [[ -z "${SYSROOT:-}" ]]; then
        echo "Error: SYSROOT not set in $cfg" >&2
        return 1
    fi

    # Source optional board.sh for function overrides
    local board_script="$BASE_DIR/boards/$board/board.sh"
    if [[ -f "$board_script" ]]; then
        # shellcheck source=/dev/null
        source "$board_script"
    fi
}

# Apply per-package CFLAGS workarounds to a crossdev sysroot
# Reads WORKAROUND_PKGS and WORKAROUND_CFLAGS arrays from board.conf
apply_workarounds() {
    local sysroot_host_path="$1"

    [[ -z "${WORKAROUND_PKGS+x}" ]] && return 0
    [[ ${#WORKAROUND_PKGS[@]} -eq 0 ]] && return 0
    [[ ${#WORKAROUND_PKGS[@]} -ne ${#WORKAROUND_CFLAGS[@]} ]] && {
        echo "Error: WORKAROUND_PKGS and WORKAROUND_CFLAGS length mismatch" >&2; return 1; }

    mkdir -p "$sysroot_host_path/etc/portage/env"
    mkdir -p "$sysroot_host_path/etc/portage/package.env"

    local i pkg cflags env_name
    for ((i = 0; i < ${#WORKAROUND_PKGS[@]}; i++)); do
        pkg="${WORKAROUND_PKGS[$i]}"
        cflags="${WORKAROUND_CFLAGS[$i]}"
        env_name="${pkg##*/}"

        cat > "$sysroot_host_path/etc/portage/env/${env_name}.conf" << EOF
CFLAGS="${cflags}"
CXXFLAGS="${cflags}"
EOF
        echo "$pkg ${env_name}.conf" >> "$sysroot_host_path/etc/portage/package.env/workarounds"
    done
    echo "Applied ${#WORKAROUND_PKGS[@]} CFLAGS workaround(s) for $BOARD_NAME"
}

list_boards() {
    for conf in "$BASE_DIR"/boards/*/board.conf; do
        [[ -f "$conf" ]] || continue
        basename "$(dirname "$conf")"
    done
}
