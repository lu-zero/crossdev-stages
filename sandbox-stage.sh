#! /bin/bash
set -euo pipefail
# More structured sandbox prototype
#
# - cache the stage3 in a .cache path
# - unpack either in a known place in $CACHE_DIR/sandboxes/ or as needed
# - provide an enter/run command that by default uses the latest sandbox

CACHE_DIR="${HOME}/.cache/crossdev-stages"
STAGES_DIR="${CACHE_DIR}/stages"
SANDBOXES_DIR="${CACHE_DIR}/sandboxes"
TARGETS_DIR="${CACHE_DIR}/targets"
BUILDS_DIR="${CACHE_DIR}/builds"
LDCONFIG="/usr/local/bin/ldconfig"
BASE_DIR=$(dirname "$(readlink -f "$0")")

ensure_cache_dirs() {
    mkdir -p "$STAGES_DIR"
    mkdir -p "$SANDBOXES_DIR"
    mkdir -p "$TARGETS_DIR"
    mkdir -p "$BUILDS_DIR"
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

    if [[ -n "${opt_mirror:-}" ]]; then
        set_make_conf_var "$make_conf" "GENTOO_MIRRORS" "${opt_mirror}"
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
    run "$sandbox_dir" "eselect repository list -i | grep -q crossdev || eselect repository create crossdev"

    # Initialize crossdev for target architecture
    run "$sandbox_dir" crossdev "${chost}" --init-target

    # Add rust-std workaround
    run "$sandbox_dir" "echo \"cross-${target_arch}-unknown-linux-gnu/rust-std **\" > /etc/portage/package.accept_keywords/rust-std"

    # Enable gcc-16 prerelease
    run "$sandbox_dir" "echo \"<sys-devel/gcc-16.0.9999:16 **\" > /etc/portage/package.accept_keywords/gcc"

    # Ensure consistent gcc versions for host and cross compilers
    # Use the latest gcc-16 snapshot for both to avoid version mismatches
    run "$sandbox_dir" "emerge -b -k sys-devel/gcc:16"

    # Get the latest gcc-16 version and set it for both host and cross
    local gcc_16_version
    gcc_16_version=$(run "$sandbox_dir" "qlist -ICev sys-devel/gcc:16 | head -n1 | sed 's|.*/gcc-||'")

    # Set the host compiler to use gcc-16
    local gcc_16_profile=$(run "$sandbox_dir" "gcc-config -l | grep '16' | head -n1 | awk '{print \$2}'")
    run "$sandbox_dir" "gcc-config ${gcc_16_profile}"
    run "$sandbox_dir" "source /etc/profile && env-update"

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

    if [[ -n "${opt_mirror:-}" ]]; then
        set_make_conf_var "$host_make_conf" "GENTOO_MIRRORS" "${opt_mirror}"
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

    # Install crossdev toolchain with the same gcc version as host
    run "$sandbox_dir" crossdev "${chost}" --gcc "${gcc_16_version}" --ex-pkg sys-devel/clang-crossdev-wrappers --ex-pkg sys-devel/rust-std

    # Switch cross compiler to gcc-16 (crossdev may leave gcc-15 as default)
    run "$sandbox_dir" "gcc-config ${chost}-16 && source /etc/profile"

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
        "sys-fs/dosfstools"
        "sys-fs/mtools"
        "app-eselect/eselect-repository"
        "dev-lang/rust"
        "sys-kernel/gentoo-sources"
    )

    echo "Syncing portage tree..."
    run "$sandbox_dir" emerge-webrsync

    echo "Running getuto..."
    run "$sandbox_dir" getuto || true

    echo "Emerging bin dependencies..."
    run "$sandbox_dir" emerge -G "${bin_packages[@]}"
    echo "Emerging dependencies..."
    run "$sandbox_dir" emerge -b -k "${packages[@]}"

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
# Board-specific BOARD_CFLAGS (from board.conf) takes precedence
target_cflags() {
    local arch=$1
    if [[ -n "${BOARD_CFLAGS:-}" ]]; then
        echo "$BOARD_CFLAGS"
        return
    fi
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
      --tmpfs /dev/shm \
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
      --tmpfs /dev/shm \
      -e TERM="$TERM" \
      -e COLORTERM="${COLORTERM:-}" \
      -e NO_COLOR="${NO_COLOR:-}" \
      -e HOME=/root \
      -e CONFIG_CHECK="" \
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
      --tmpfs /dev/shm \
      -B "$stage_dir":/target \
      -e TERM="$TERM" \
      -e COLORTERM="${COLORTERM:-}" \
      -e NO_COLOR="${NO_COLOR:-}" \
      -e HOME=/root \
      -e CONFIG_CHECK="" \
      -- bash --login -c "
         $args
      "
}

run_with_build() {
    local sandbox_dir="$1"
    local build_dir="$2"
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
      --tmpfs /dev/shm \
      -B "$build_dir":/build \
      -b "$BASE_DIR":/scripts \
      -e TERM="$TERM" \
      -e COLORTERM="${COLORTERM:-}" \
      -e NO_COLOR="${NO_COLOR:-}" \
      -e HOME=/root \
      -e CONFIG_CHECK="" \
      -- bash --login -c "
         $args
      "
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
      --tmpfs /dev/shm \
      -B "$build_dir":/build \
      -b "$source_dir":/target_src \
      -b "$BASE_DIR":/scripts \
      "${extra_args[@]}" \
      -e TERM="$TERM" \
      -e COLORTERM="${COLORTERM:-}" \
      -e NO_COLOR="${NO_COLOR:-}" \
      -e HOME=/root \
      -e CONFIG_CHECK="" \
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
      --allow-new-privs \
      --userns=auto \
      --tmpfs /dev/shm \
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

prepare_target_portage() {
    local sandbox_dir="$1"
    local target_dir="$2"
    local target_arch="$3"

    gentoo_arch "$target_arch"
    local chost="${target_arch}-unknown-linux-gnu"
    local cflags
    cflags=$(target_cflags "$target_arch")
    local profile
    profile=$(gentoo_profile "$target_arch")

    mkdir -p "$target_dir/etc/portage"

    cat > "$target_dir/etc/portage/make.conf" << EOF
CHOST="${chost}"
ACCEPT_KEYWORDS="~${ARCH}"
CFLAGS="${cflags}"
CXXFLAGS="\${CFLAGS}"
EOF

    # Copy profile link from crossdev sysroot
    local crossdev_root="$sandbox_dir/usr/${chost}"
    if [[ -d "$crossdev_root/etc/portage/profile" ]]; then
        cp -a "$crossdev_root/etc/portage/profile" "$target_dir/etc/portage/"
    fi
    if [[ -L "$crossdev_root/etc/portage/make.profile" ]]; then
        cp -a "$crossdev_root/etc/portage/make.profile" "$target_dir/etc/portage/"
    fi
}

build_stage1() {
    local sandbox_dir="$1"
    local target_dir="$2"
    local target_arch="${3:-riscv64}"
    local chost="${target_arch}-unknown-linux-gnu"

    echo "Building stage1 for ${chost} from scratch..."

    # Prepare target portage configuration
    prepare_target_portage "$sandbox_dir" "$target_dir" "$target_arch"

    # Step 1: baselayout (directory skeleton)
    echo "==> Installing baselayout..."
    run_with_stage "$sandbox_dir" "$target_dir" \
        "USE=build ROOT=/target ${chost}-emerge -b -k sys-apps/baselayout"

    # Step 2: packages.build (core system)
    echo "==> Installing stage1 packages..."
    local packages
    packages=$(run "$sandbox_dir" \
        "grep -v '^#' /var/db/repos/gentoo/profiles/default/linux/packages.build | grep -v '^\$' | tr '\n' ' '")
    run_with_stage "$sandbox_dir" "$target_dir" \
        "ROOT=/target ${chost}-emerge -b -k ${packages}"

    # Step 3: portage
    echo "==> Installing portage..."
    run_with_stage "$sandbox_dir" "$target_dir" \
        "USE=build ROOT=/target ${chost}-emerge -b -k sys-apps/portage"

    update_ldconfig_sandbox "$sandbox_dir" "$target_dir"
    echo "$(date -u +%Y%m%dT%H%M%SZ) stage1" >> "$target_dir/.updated"
    echo "Stage1 build complete for ${chost}"
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

# resolve_dir <nameref> <base_dir> <get_latest_fn> <label> [arg]
# Sets <nameref> to base_dir/arg, or to the latest entry if arg is absent or "latest".
# Caller should follow with: [[ $# -gt 0 ]] && shift
resolve_dir() {
    local -n _rd_out=$1
    local base=$2
    local fn=$3
    local label=$4
    local arg="${5-}"

    if [[ -z "$arg" || "$arg" == "latest" ]]; then
        _rd_out=$("$fn")
        if [[ -z "$_rd_out" ]]; then
            echo "Error: No $label found." >&2
            exit 1
        fi
    else
        _rd_out="$base/$arg"
    fi
}

resolve_build()  { resolve_dir "$1" "$BUILDS_DIR"  get_latest_build  "build"  "${2-}"; }
resolve_target() { resolve_dir "$1" "$TARGETS_DIR" get_latest_target "target" "${2-}"; }

load_board_config() {
    local board="$1"
    local cfg="$BASE_DIR/boards/$board/board.conf"
    [[ -f "$cfg" ]] || { echo "Board config not found: $cfg" >&2; return 1; }
    # shellcheck source=/dev/null
    source "$cfg"
    BOARD_CFG_DIR="$BASE_DIR/boards/$board"

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
    local sandbox_dir="$1"
    local target_arch="$2"
    local chost="${target_arch}-unknown-linux-gnu"
    local crossdev_root="/usr/${chost}"
    local host_crossdev_root="$sandbox_dir${crossdev_root}"

    [[ -z "${WORKAROUND_PKGS+x}" ]] && return 0
    [[ ${#WORKAROUND_PKGS[@]} -eq 0 ]] && return 0

    mkdir -p "$host_crossdev_root/etc/portage/env"
    mkdir -p "$host_crossdev_root/etc/portage/package.env"

    local i pkg cflags env_name
    for ((i = 0; i < ${#WORKAROUND_PKGS[@]}; i++)); do
        pkg="${WORKAROUND_PKGS[$i]}"
        cflags="${WORKAROUND_CFLAGS[$i]}"
        env_name="${pkg##*/}"

        cat > "$host_crossdev_root/etc/portage/env/${env_name}.conf" << EOF
CFLAGS="${cflags}"
CXXFLAGS="${cflags}"
EOF
        echo "$pkg ${env_name}.conf" >> "$host_crossdev_root/etc/portage/package.env/workarounds"
    done
    echo "Applied ${#WORKAROUND_PKGS[@]} CFLAGS workaround(s) for $BOARD_NAME"
}

image_install_deps() {
    local sandbox_dir="$1"
    local target_dir="$2"
    local board="$3"
    load_board_config "$board"

    local sandbox_pkgs="$BOARD_CFG_DIR/sandbox-packages.txt"
    local target_pkgs="$BOARD_CFG_DIR/target-packages.txt"
    local target_arch="$BOARD_ARCH"
    local chost="${target_arch}-unknown-linux-gnu"

    # Apply per-package CFLAGS workarounds before emerging
    apply_workarounds "$sandbox_dir" "$target_arch"

    if [[ -f "$sandbox_pkgs" ]]; then
        echo "Installing sandbox packages for $board..."
        # shellcheck disable=SC2046
        run "$sandbox_dir" emerge -b -k $(packages_from_file "$sandbox_pkgs")
    fi

    if [[ -f "$target_pkgs" && -n "$target_dir" ]]; then
        echo "Cross-installing target packages for $board..."
        install_packages "$sandbox_dir" "$target_dir" "$target_arch" \
            $(packages_from_file "$target_pkgs")
    elif [[ -f "$target_pkgs" ]]; then
        echo "Skipping target packages (no target sysroot available)"
    fi
}

image_checkout() {
    local sandbox_dir="$1"
    local build_dir="$2"
    local board="$3"
    load_board_config "$board"

    echo "Checking out sources for $board into $build_dir..."
    if type -t board_checkout &>/dev/null; then
        board_checkout "$sandbox_dir" "$build_dir"
    else
        run_with_build "$sandbox_dir" "$build_dir" "
            checkout() {
                local repo=\$1 tag=\$2 src=/build/\$3
                if [[ -d \"\$src\" ]]; then
                    (cd \"\$src\" && git fetch && git checkout \"\$tag\")
                else
                    git clone --depth 1 --branch \"\$tag\" \"\$repo\" \"\$src\"
                fi
            }
            checkout '${OPENSBI_REPO}' '${OPENSBI_TAG}' opensbi
            checkout '${U_BOOT_REPO}' '${TAG}' u-boot
            checkout '${KERNEL_REPO}' '${TAG}' linux
            checkout '${FIRMWARE_REPO}' '${TAG}' firmware
        "
    fi
    echo "$(date -u +%Y%m%dT%H%M%SZ)" > "$build_dir/.sources"
}

image_build_bootloader() {
    local sandbox_dir="$1"
    local build_dir="$2"
    local board="$3"
    load_board_config "$board"

    echo "Building bootloader for $board..."
    if type -t board_build_bootloader &>/dev/null; then
        board_build_bootloader "$sandbox_dir" "$build_dir"
    else
        run_with_build "$sandbox_dir" "$build_dir" "
            make -C /build/opensbi PLATFORM=${OPENSBI_PLATFORM} PLATFORM_DEFCONFIG=defconfig CROSS_COMPILE=${CROSS_COMPILE} -j\$(nproc)
            make -C /build/u-boot ARCH=${KERNEL_ARCH} CROSS_COMPILE=${CROSS_COMPILE} ${U_BOOT_DEFCONFIG}
            make -C /build/u-boot ARCH=${KERNEL_ARCH} CROSS_COMPILE=${CROSS_COMPILE} -j\$(nproc)
        "
    fi
    echo "$(date -u +%Y%m%dT%H%M%SZ)" > "$build_dir/.bootloader"
}

image_build_kernel() {
    local sandbox_dir="$1"
    local build_dir="$2"
    local board="$3"
    load_board_config "$board"

    echo "Building kernel for $board..."
    if type -t board_build_kernel &>/dev/null; then
        board_build_kernel "$sandbox_dir" "$build_dir"
    else
        run_with_build "$sandbox_dir" "$build_dir" "
            make -C /build/linux ARCH=${KERNEL_ARCH} CROSS_COMPILE=${CROSS_COMPILE} ${KERNEL_DEFCONFIG}
            make -C /build/linux ARCH=${KERNEL_ARCH} CROSS_COMPILE=${CROSS_COMPILE} -j\$(nproc)
            make -C /build/linux ARCH=${KERNEL_ARCH} CROSS_COMPILE=${CROSS_COMPILE} modules -j\$(nproc)
        "
    fi
    echo "$(date -u +%Y%m%dT%H%M%SZ)" > "$build_dir/.kernel"
}

image_assemble() {
    local sandbox_dir="$1"
    local build_dir="$2"
    local source_dir="$3"
    local board="$4"
    load_board_config "$board"

    # Build extra bind args for host firmware paths
    local extra_args=()
    for fw_path in "${HOST_FIRMWARE_PATHS[@]+"${HOST_FIRMWARE_PATHS[@]}"}"; do
        [[ -d "$fw_path" ]] && extra_args+=("-b" "${fw_path}:${fw_path}")
    done

    # Pre-compute service setup commands (runs inside sandbox)
    local svc_cmds=""
    for svc_pair in "${BOOT_SERVICES[@]+"${BOOT_SERVICES[@]}"}"; do
        local svc="${svc_pair%%:*}" lvl="${svc_pair##*:}"
        svc_cmds+="ln -sf /etc/init.d/${svc} /build/gen/root/etc/runlevels/${lvl}/; "
    done

    # Pre-compute host firmware copy commands
    local fw_cmds=""
    for fw_path in "${HOST_FIRMWARE_PATHS[@]+"${HOST_FIRMWARE_PATHS[@]}"}"; do
        fw_cmds+="cp -a '${fw_path}' /build/gen/root/lib/firmware/ 2>/dev/null || true; "
    done

    echo "Assembling image for $board from $source_dir..."
    run_with_build_and_source "$sandbox_dir" "$build_dir" "$source_dir" -- "
        set -e
        mkdir -p /build/gen/root /build/gen/boot

        # Copy target sysroot into build area
        cp -a /target_src/. /build/gen/root/

        # Install kernel modules into root
        INSTALL_MOD_PATH=/build/gen/root make -C /build/linux ARCH=${KERNEL_ARCH} CROSS_COMPILE=${CROSS_COMPILE} modules_install

        # Enable services
        mkdir -p /build/gen/root/etc/runlevels/{boot,default,nonetwork,shutdown,sysinit}
        ${svc_cmds}

        # System configuration
        mkdir -p /build/gen/root/etc/conf.d
        echo 'hostname=\"${BOOT_HOSTNAME}\"' > /build/gen/root/etc/conf.d/hostname
        echo 'x1:12345:respawn:/sbin/agetty ${BOOT_SERIAL_BAUD} ${BOOT_SERIAL_TTY} linux' >> /build/gen/root/etc/inittab
        sed -i -e 's/root:x:/root::/' /build/gen/root/etc/passwd
        mkdir -p /build/gen/root/etc/ssh
        printf 'PermitRootLogin yes\nPermitEmptyPasswords yes\nStrictModes yes\n' >> /build/gen/root/etc/ssh/sshd_config

        # Update ldconfig in assembled root
        ${LDCONFIG} -v -r /build/gen/root
    "

    # Board-specific assembly (firmware, boot image, initramfs)
    if type -t board_assemble &>/dev/null; then
        board_assemble "$sandbox_dir" "$build_dir" "$source_dir"
    else
        # Default: copy DTBs, kernel, firmware overlay, host firmware
        run_with_build_and_source "$sandbox_dir" "$build_dir" "$source_dir" \
          ${extra_args[@]+"${extra_args[@]}"} -- "
            set -e
            cp /build/linux/${BOARD_DTB_GLOB} /build/gen/boot/
            mkdir -p /build/gen/root/lib/firmware
            cp -a /build/firmware/${BOARD_FIRMWARE_OVERLAY}/. /build/gen/root/lib/firmware/
            ${fw_cmds}
            cp /build/linux/arch/${KERNEL_ARCH}/boot/${BOOT_KERNEL_NAME} /build/gen/boot/
        "
    fi

    echo "$(date -u +%Y%m%dT%H%M%SZ)" >> "$build_dir/.assembled"
}

image_pack() {
    local sandbox_dir="$1"
    local build_dir="$2"
    local board="$3"
    local compress="${4:-xz}"
    load_board_config "$board"

    local img_name="${IMAGE_NAME:-gentoo-linux-${BOARD_NAME}_dev-sdcard.img}"

    echo "Packing image for $board..."
    run_with_build "$sandbox_dir" "$build_dir" "
        rm -rf /build/tmp
        cd /build
        genimage --config /scripts/boards/${board}/genimage.cfg \
            --inputpath /build \
            --outputpath /build \
            --rootpath /build/gen
    "

    if [[ "$compress" == "none" ]]; then
        echo "Image ready: $build_dir/$img_name"
    else
        run_with_build "$sandbox_dir" "$build_dir" \
            "xz -f -T0 -9 /build/$img_name"
        echo "Image ready: $build_dir/$img_name.xz"
    fi
    echo "$(date -u +%Y%m%dT%H%M%SZ)" > "$build_dir/.packed"
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

require_args() {
    local n="$1"
    local msg="$2"
    shift 2
    if [[ $# -lt $n ]]; then
        echo "Error: $msg" >&2
        usage
    fi
}

usage() {
    echo "$0 [-s|--sandbox <name>] [-m|--mirror <url>] <command> [options]"
    echo ""
    echo "Global options:"
    echo "  -s, --sandbox <name>           - Use named sandbox (default: latest)"
    echo "  -m, --mirror <url>             - Set Gentoo mirror (e.g. https://ftp.kaist.ac.kr/gentoo/)"
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
    echo "  $0 target setup [arch] [name]  - Setup target sysroot for arch (from stage3)"
    echo "  $0 target build-stage1 [arch] [name] - Build stage1 from scratch via crossdev"
    echo "  $0 target destroy <name>       - Remove a target"
    echo "  $0 target update [target] [arch] - Update target via cross-emerge"
    echo "  $0 target pack [target] [arch] - Pack target as stage3 tarball in stages cache"
    echo ""
    echo "Package install commands:"
    echo "  $0 install [target] [arch] pkg... - Install packages into target"
    echo "  $0 install-from [target] [arch] file - Install packages from file"
    echo "  $0 update-ldconfig [target]    - Regenerate ld.so.cache in target"
    echo ""
    echo "Image build commands:"
    echo "  $0 image boards                - List available boards"
    echo "  $0 image list                  - List builds (name, board, state)"
    echo "  $0 image destroy <name>        - Remove a build"
    echo "  $0 image setup <board> [name]  - Create named build dir for board"
    echo "  $0 image install-deps <board> [target] - Install sandbox + target packages for board"
    echo "  $0 image checkout [build]      - Clone/update source repos"
    echo "  $0 image build-boot [build]    - Build OpenSBI + u-boot"
    echo "  $0 image build-kernel [build]  - Build Linux kernel + modules"
    echo "  $0 image assemble [build] [target] - Copy rootfs, install modules+firmware, create initramfs"
    echo "  $0 image pack [build]          - Run genimage + xz compress (--no-compress to skip xz)"
    echo "  $0 image build <board> [name] [target] - Full pipeline (order from BUILD_STEPS in board.conf)"
    echo "  $0 --dry-run image build <board>      - Show board config and build steps without building"
    echo ""
    echo "Maintenance:"
    echo "  $0 prune [board]               - Remove incomplete builds (keep packed ones)"
    echo "  $0 prune [board] --all         - Remove all builds (for board if specified)"
    echo ""
    echo "Cache directory: $CACHE_DIR"
    exit 1
}

main() {
    [[ $# -gt 0 ]] || usage

    # Parse global flags before the command
    local opt_sandbox=""
    local opt_mirror=""
    local opt_compress="xz"
    local opt_dry_run=0
    local filtered_args=()
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --sandbox|-s) opt_sandbox="$2"; shift 2 ;;
            --mirror|-m) opt_mirror="$2"; shift 2 ;;
            --no-compress) opt_compress="none"; shift ;;
            --dry-run) opt_dry_run=1; shift ;;
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
            [[ $# -gt 0 ]] && shift
            local sandbox_name="${1:-$arch}"
            [[ $# -gt 0 ]] && shift

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
            [[ -z "$opt_sandbox" ]] && require_args 1 "destroy requires a sandbox name" "$@"
            local name="${1:-$opt_sandbox}"
            local target="$SANDBOXES_DIR/$name"
            if [[ ! -d "$target" ]]; then
                echo "Error: Sandbox not found: $name" >&2
                exit 1
            fi
            echo "Removing sandbox: $name"
            remove_dir "$target"
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
            [[ $# -gt 0 ]] && shift

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
                    require_args 1 "target destroy requires a target name" "$@"
                    local target="$TARGETS_DIR/$1"
                    if [[ ! -d "$target" ]]; then
                        echo "Error: Target not found: $1" >&2
                        exit 1
                    fi
                    echo "Removing target: $1"
                    remove_dir "$target"
                    echo "Target $1 removed."
                    ;;
                setup)
                    local arch="${1:-$(uname -m)}"
                    [[ $# -gt 0 ]] && shift
                    local target_name="${1:-$arch}"
                    [[ $# -gt 0 ]] && shift

                    ensure_target "$arch" "$target_name" || exit 1
                    ;;
                build-stage1)
                    local arch="${1:-riscv64}"
                    [[ $# -gt 0 ]] && shift
                    local target_name="${1:-${arch}-stage1}"
                    [[ $# -gt 0 ]] && shift

                    local target_dir="$TARGETS_DIR/$target_name"
                    mkdir -p "$target_dir"
                    echo "$arch" > "$target_dir/.arch"

                    local sandbox_dir
                    sandbox_dir=$(resolve_sandbox)
                    ensure_crossdev "$sandbox_dir" "$arch" || exit 1
                    build_stage1 "$sandbox_dir" "$target_dir" "$arch"
                    ;;
                update)
                    local target_dir
                    resolve_target target_dir "${1-}"
                    [[ $# -gt 0 ]] && shift

                    local target_arch="${1:-$(get_arch "$target_dir")}"
                    [[ $# -gt 0 ]] && shift
                    [[ -z "$target_arch" ]] && { echo "Error: Cannot determine target arch. Specify explicitly." >&2; exit 1; }

                    local target_name=$(basename "$target_dir")
                    ensure_target "$target_arch" "$target_name" || exit 1

                    local sandbox_dir
                    sandbox_dir=$(resolve_sandbox)
                    update_stage3 "$sandbox_dir" "$target_dir" "$target_arch"
                    ;;
                pack)
                    local target_dir
                    resolve_target target_dir "${1-}"
                    [[ $# -gt 0 ]] && shift

                    local target_arch="${1:-$(get_arch "$target_dir")}"
                    [[ $# -gt 0 ]] && shift
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
                      --tmpfs /dev/shm \
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
            local target_dir
            resolve_target target_dir "${1-}"
            [[ $# -gt 0 ]] && shift

            local target_arch="${1:-$(get_arch "$target_dir")}"; [[ $# -gt 0 ]] && shift
            [[ -z "$target_arch" ]] && { echo "Error: Cannot determine target arch. Specify explicitly." >&2; exit 1; }

            local sandbox_dir
            sandbox_dir=$(resolve_sandbox)
            [[ -z "$sandbox_dir" ]] && { echo "Error: No sandbox found." >&2; exit 1; }

            [[ ! -d "$sandbox_dir" ]] && { echo "Error: Sandbox not found: $sandbox_dir" >&2; exit 1; }
            [[ ! -d "$target_dir" ]] && { echo "Error: Target not found: $target_dir" >&2; exit 1; }

            install_packages "$sandbox_dir" "$target_dir" "$target_arch" "$@"
            ;;
        install-from)
            local target_dir
            resolve_target target_dir "${1-}"
            [[ $# -gt 0 ]] && shift

            local target_arch="${1:-$(get_arch "$target_dir")}"; [[ $# -gt 0 ]] && shift
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
            local target_dir
            resolve_target target_dir "${1-}"
            [[ $# -gt 0 ]] && shift

            local sandbox_dir
            sandbox_dir=$(resolve_sandbox)
            [[ -z "$sandbox_dir" ]] && { echo "Error: No sandbox found." >&2; exit 1; }

            [[ ! -d "$sandbox_dir" ]] && { echo "Error: Sandbox not found: $sandbox_dir" >&2; exit 1; }
            [[ ! -d "$target_dir" ]] && { echo "Error: Target not found: $target_dir" >&2; exit 1; }

            update_ldconfig_sandbox "$sandbox_dir" "$target_dir"
            ;;
        image)
            local subcmd="${1:-list}"
            shift

            case $subcmd in
                boards)
                    for conf in "$BASE_DIR"/boards/*/board.conf; do
                        [[ -f "$conf" ]] || continue
                        local bdir bname barch
                        bdir=$(dirname "$conf")
                        bname=$(basename "$bdir")
                        barch=$(. "$conf" && echo "$BOARD_ARCH")
                        printf "%-15s %s\n" "$bname" "$barch"
                    done
                    ;;
                list)
                    if [[ -d "$BUILDS_DIR" ]]; then
                        for d in "$BUILDS_DIR"/*/; do
                            [[ -d "$d" ]] || continue
                            local name=$(basename "$d")
                            local board=$(get_build_board "$d")
                            local steps=()
                            [[ -f "$d/.deps"       ]] && steps+=("deps")
                            [[ -f "$d/.sources"    ]] && steps+=("sources")
                            [[ -f "$d/.bootloader" ]] && steps+=("bootloader")
                            [[ -f "$d/.kernel"     ]] && steps+=("kernel")
                            [[ -f "$d/.assembled"  ]] && steps+=("assembled($(cat "$d/.assembled"))")
                            [[ -f "$d/.packed"     ]] && steps+=("packed")
                            local state="setup"
                            [[ ${#steps[@]} -gt 0 ]] && state="${steps[*]}"
                            printf "%-30s %-10s %s\n" "$name" "${board:-(unknown)}" "$state"
                        done
                    else
                        echo "No builds found."
                    fi
                    ;;
                destroy)
                    require_args 1 "image destroy requires a build name" "$@"
                    local target="$BUILDS_DIR/$1"
                    if [[ ! -d "$target" ]]; then
                        echo "Error: Build not found: $1" >&2
                        exit 1
                    fi
                    echo "Removing build: $1"
                    remove_dir "$target"
                    echo "Build $1 removed."
                    ;;
                setup)
                    require_args 1 "image setup requires a board name" "$@"
                    local board="$1"; shift
                    local timestamp
                    timestamp=$(date -u +%Y%m%dT%H%M%SZ)
                    local build_name="${1:-${board}-${timestamp}}"
                    [[ $# -gt 0 ]] && shift

                    ensure_cache_dirs
                    local build_dir="$BUILDS_DIR/$build_name"
                    mkdir -p "$build_dir"
                    echo "$board" > "$build_dir/.board"
                    echo "Build ready: $build_dir (board: $board)"
                    ;;
                install-deps)
                    require_args 1 "image install-deps requires a board name" "$@"
                    local board="$1"; shift

                    local target_dir=""
                    if [[ $# -gt 0 ]]; then
                        resolve_target target_dir "$1"; shift
                    else
                        target_dir=$(get_latest_target) || true
                    fi

                    local sandbox_dir
                    sandbox_dir=$(resolve_sandbox)
                    [[ -z "$sandbox_dir" || ! -d "$sandbox_dir" ]] && { echo "Error: No sandbox found." >&2; exit 1; }

                    image_install_deps "$sandbox_dir" "$target_dir" "$board"
                    ;;
                checkout)
                    local build_dir
                    resolve_build build_dir "${1-}"
                    [[ $# -gt 0 ]] && shift

                    local board
                    board=$(get_build_board "$build_dir")
                    [[ -z "$board" ]] && { echo "Error: No .board metadata in $build_dir" >&2; exit 1; }

                    local sandbox_dir
                    sandbox_dir=$(resolve_sandbox)
                    [[ -z "$sandbox_dir" || ! -d "$sandbox_dir" ]] && { echo "Error: No sandbox found." >&2; exit 1; }

                    image_checkout "$sandbox_dir" "$build_dir" "$board"
                    ;;
                build-boot)
                    local build_dir
                    resolve_build build_dir "${1-}"
                    [[ $# -gt 0 ]] && shift

                    local board
                    board=$(get_build_board "$build_dir")
                    [[ -z "$board" ]] && { echo "Error: No .board metadata in $build_dir" >&2; exit 1; }

                    local sandbox_dir
                    sandbox_dir=$(resolve_sandbox)
                    [[ -z "$sandbox_dir" || ! -d "$sandbox_dir" ]] && { echo "Error: No sandbox found." >&2; exit 1; }

                    image_build_bootloader "$sandbox_dir" "$build_dir" "$board"
                    ;;
                build-kernel)
                    local build_dir
                    resolve_build build_dir "${1-}"
                    [[ $# -gt 0 ]] && shift

                    local board
                    board=$(get_build_board "$build_dir")
                    [[ -z "$board" ]] && { echo "Error: No .board metadata in $build_dir" >&2; exit 1; }

                    local sandbox_dir
                    sandbox_dir=$(resolve_sandbox)
                    [[ -z "$sandbox_dir" || ! -d "$sandbox_dir" ]] && { echo "Error: No sandbox found." >&2; exit 1; }

                    image_build_kernel "$sandbox_dir" "$build_dir" "$board"
                    ;;
                assemble)
                    local build_dir
                    resolve_build build_dir "${1-}"
                    [[ $# -gt 0 ]] && shift

                    local target_dir
                    resolve_target target_dir "${1-}"
                    [[ $# -gt 0 ]] && shift

                    local board
                    board=$(get_build_board "$build_dir")
                    [[ -z "$board" ]] && { echo "Error: No .board metadata in $build_dir" >&2; exit 1; }

                    local sandbox_dir
                    sandbox_dir=$(resolve_sandbox)
                    [[ -z "$sandbox_dir" || ! -d "$sandbox_dir" ]] && { echo "Error: No sandbox found." >&2; exit 1; }

                    image_assemble "$sandbox_dir" "$build_dir" "$target_dir" "$board"
                    ;;
                pack)
                    local build_dir
                    resolve_build build_dir "${1-}"
                    [[ $# -gt 0 ]] && shift

                    local board
                    board=$(get_build_board "$build_dir")
                    [[ -z "$board" ]] && { echo "Error: No .board metadata in $build_dir" >&2; exit 1; }

                    local sandbox_dir
                    sandbox_dir=$(resolve_sandbox)
                    [[ -z "$sandbox_dir" || ! -d "$sandbox_dir" ]] && { echo "Error: No sandbox found." >&2; exit 1; }

                    image_pack "$sandbox_dir" "$build_dir" "$board" "$opt_compress"
                    ;;
                build)
                    require_args 1 "image build requires a board name" "$@"
                    local board="$1"; shift

                    load_board_config "$board"
                    if [[ -z "${BUILD_STEPS+x}" ]]; then
                        BUILD_STEPS=(deps checkout bootloader kernel assemble pack)
                    fi
                    local steps=("${BUILD_STEPS[@]}")
                    local total=${#steps[@]}

                    if [[ $opt_dry_run -eq 1 ]]; then
                        echo "Board:      $BOARD_NAME"
                        echo "Arch:       $BOARD_ARCH"
                        echo "CFLAGS:     ${BOARD_CFLAGS:-$(target_cflags "$BOARD_ARCH")}"
                        echo "Steps:      ${steps[*]}"
                        echo "Image:      ${IMAGE_NAME:-gentoo-linux-${BOARD_NAME}_dev-sdcard.img}"
                        for step in "${steps[@]}"; do
                            if type -t "board_build_${step}" &>/dev/null || type -t "board_${step}" &>/dev/null; then
                                echo "  $step: board override"
                            else
                                echo "  $step: default"
                            fi
                        done
                    else
                        local timestamp
                        timestamp=$(date -u +%Y%m%dT%H%M%SZ)
                        local build_name="${1:-${board}-${timestamp}}"
                        [[ $# -gt 0 ]] && shift

                        ensure_cache_dirs
                        local build_dir="$BUILDS_DIR/$build_name"
                        mkdir -p "$build_dir"
                        echo "$board" > "$build_dir/.board"

                        local target_dir
                        resolve_target target_dir "${1-}"
                        [[ $# -gt 0 ]] && shift

                        local sandbox_dir
                        sandbox_dir=$(resolve_sandbox)
                        [[ -z "$sandbox_dir" || ! -d "$sandbox_dir" ]] && { echo "Error: No sandbox found." >&2; exit 1; }

                        local step_num=0
                        for step in "${steps[@]}"; do
                            step_num=$((step_num + 1))
                            echo "==> [$step_num/$total] ${step}..."
                            case "$step" in
                                deps)
                                    image_install_deps "$sandbox_dir" "$target_dir" "$board"
                                    echo "$(date -u +%Y%m%dT%H%M%SZ)" > "$build_dir/.deps"
                                    ;;
                                checkout)
                                    image_checkout "$sandbox_dir" "$build_dir" "$board"
                                    ;;
                                bootloader)
                                    image_build_bootloader "$sandbox_dir" "$build_dir" "$board"
                                    ;;
                                kernel)
                                    image_build_kernel "$sandbox_dir" "$build_dir" "$board"
                                    ;;
                                assemble)
                                    image_assemble "$sandbox_dir" "$build_dir" "$target_dir" "$board"
                                    ;;
                                pack)
                                    image_pack "$sandbox_dir" "$build_dir" "$board" "$opt_compress"
                                    ;;
                                *)
                                    echo "Error: Unknown build step: $step" >&2
                                    exit 1
                                    ;;
                            esac
                        done
                    fi
                    ;;
                *)
                    usage
                    ;;
            esac
            ;;
        prune)
            local prune_all=0
            local prune_board=""
            while [[ $# -gt 0 ]]; do
                case "$1" in
                    --all) prune_all=1; shift ;;
                    *) prune_board="$1"; shift ;;
                esac
            done

            if [[ ! -d "$BUILDS_DIR" ]]; then
                echo "No builds to prune."
            else
                local count=0
                for d in "$BUILDS_DIR"/*/; do
                    [[ -d "$d" ]] || continue
                    local name=$(basename "$d")
                    local board=$(get_build_board "$d")

                    # Filter by board if specified
                    if [[ -n "$prune_board" && "$board" != "$prune_board" ]]; then
                        continue
                    fi

                    if [[ $prune_all -eq 1 ]] || [[ ! -f "$d/.packed" ]]; then
                        echo "Removing: $name ($board)"
                        remove_dir "$d"
                        count=$((count + 1))
                    fi
                done
                echo "Pruned $count build(s)."
            fi
            ;;
        *)
            usage
            ;;
    esac
}

main "$@"
