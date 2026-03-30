# Board config loader - source this from other scripts
#
# Usage:
#   source lib/board.sh
#   load_board k1          # by name (looks up boards/k1/board.toml)
#   load_board boards/k1/board.toml  # by path
#
# After loading, these variables are set:
#   BOARD_NAME, BOARD_ARCH, BOARD_CFLAGS
#   BOARD_PACKAGES_BOOT, BOARD_PACKAGES_EXTRA
#   BOARD_WORKAROUND_PKGS, BOARD_WORKAROUND_CFLAGS
#   BOARD_IMAGE_NAME
#   BOARD_DIR (directory containing the board config)

_BOARD_LIB_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
_BOARD_BASE_DIR=$(dirname "$_BOARD_LIB_DIR")

load_board() {
    local config="$1"

    # Resolve board name to TOML path
    if [[ "$config" == *.toml ]]; then
        # Direct path
        if [[ ! -f "$config" ]]; then
            echo "Error: Board config not found: $config" >&2
            return 1
        fi
    else
        # Board name - look up in boards/
        config="$_BOARD_BASE_DIR/boards/$config/board.toml"
        if [[ ! -f "$config" ]]; then
            echo "Error: Unknown board: $1 (no $config)" >&2
            return 1
        fi
    fi

    BOARD_DIR=$(dirname "$config")

    local _board_env
    _board_env=$(python3 "$_BOARD_LIB_DIR/toml2env.py" "$config") || {
        echo "Error: Failed to parse $config" >&2
        return 1
    }
    eval "$_board_env"
}

# Apply per-package CFLAGS workarounds to a crossdev root
# Usage: apply_workarounds <crossdev_root>
apply_workarounds() {
    local root="$1"
    local i

    [[ ${#BOARD_WORKAROUND_PKGS[@]} -eq 0 ]] && return 0

    mkdir -p "$root/etc/portage/env" "$root/etc/portage/package.env"
    : > "$root/etc/portage/package.env/workarounds"

    for ((i = 0; i < ${#BOARD_WORKAROUND_PKGS[@]}; i++)); do
        local pkg="${BOARD_WORKAROUND_PKGS[$i]}"
        local cflags="${BOARD_WORKAROUND_CFLAGS[$i]}"
        local env_name="${pkg##*/}"

        echo "CFLAGS=\"$cflags\"" > "$root/etc/portage/env/${env_name}.conf"
        echo "CXXFLAGS=\"$cflags\"" >> "$root/etc/portage/env/${env_name}.conf"
        echo "$pkg ${env_name}.conf" >> "$root/etc/portage/package.env/workarounds"
    done
}

list_boards() {
    for toml in "$_BOARD_BASE_DIR"/boards/*/board.toml; do
        [[ -f "$toml" ]] || continue
        basename "$(dirname "$toml")"
    done
}
