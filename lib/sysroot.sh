# Sysroot management for crossdev-stages
# Source this from other scripts: source "$BASE_DIR/lib/sysroot.sh"
#
# Requires: lib/common.sh sourced first (SYSROOTS_DIR, set_make_conf_var, run)
#
# Each sysroot is a cross-compilation environment for a specific CFLAGS set.
# Created by unpacking a stage3 (generic base) and rebuilding glibc with
# board-specific CFLAGS. Only glibc needs board CFLAGS (ABI, vector routines);
# other libraries are used for link-time resolution only. Boards with the same
# SYSROOT name share a sysroot and its binary package cache (PKGDIR).
#
# hakoniwa bind-mounts the sysroot to /usr/$chost at build time.

# resolve_sysroot <sysroot_name>
# Returns the host path to the sysroot directory.
resolve_sysroot() {
    local name="$1"
    local sysroot_dir="$SYSROOTS_DIR/$name"

    if [[ -d "$sysroot_dir" && -f "$sysroot_dir/.cflags" ]]; then
        echo "$sysroot_dir"
        return 0
    fi

    echo "Sysroot '$name' not found. Create it with: $0 sysroot create $name <board>" >&2
    return 1
}

# create_sysroot <sandbox_dir> <sysroot_name> <target_arch> <cflags> [mirror]
#
# Creates a sysroot by:
# 1. Unpacking a stage3 tarball as bootstrap base (provides headers, libs)
# 2. Configuring portage with board-specific CFLAGS and PKGDIR
# 3. Rebuilding glibc with board CFLAGS (the only ABI-critical package)
#
# The stage3 provides a complete dependency graph at generic CFLAGS.
# Only glibc needs board-specific CFLAGS (ABI, vector-optimized routines).
# Other sysroot libraries are used for link-time symbol resolution only;
# the target rootfs gets its own copies via cross-emerge.
create_sysroot() {
    local sandbox_dir="$1"
    local name="$2"
    local target_arch="$3"
    local cflags="$4"
    local mirror="${5:-}"
    local sysroot_dir="$SYSROOTS_DIR/$name"
    local chost="${target_arch}-unknown-linux-gnu"
    local crossdev_root="/usr/${chost}"

    if [[ -d "$sysroot_dir" && -f "$sysroot_dir/.cflags" ]]; then
        echo "Sysroot '$name' already exists at $sysroot_dir"
        return 0
    fi

    echo "Creating sysroot '$name' for ${chost} with CFLAGS: $cflags"

    # Ensure host sandbox has crossdev toolchain
    if [[ ! -f "$sandbox_dir/.crossdev-host-${target_arch}" ]]; then
        prepare_crossdev_host "$sandbox_dir" "$target_arch"
    fi

    # Step 1: Unpack stage3 as sysroot base
    local stage_file
    stage_file=$(fetch_stage "$target_arch")
    local stage_filename
    stage_filename=$(basename "$stage_file")

    echo "==> Unpacking stage3 into sysroot..."
    hakoniwa run \
      --rootfs / --devfs /dev \
      --unshare-all --allow-new-privs --userns=auto \
      --tmpfs /tmp --tmpfs /dev/shm \
      -B "$CACHE_DIR":/cache \
      -- /bin/sh -c "
        mkdir -p '/cache/sysroots/$name' &&
        tar --overwrite -xpf '/cache/stages/$stage_filename' \
          --xattrs-include='*.*' --numeric-owner --exclude='./dev' \
          -C '/cache/sysroots/$name'
      "

    # Step 2: Configure portage with board CFLAGS
    echo "==> Configuring portage..."
    local profile
    profile=$(gentoo_profile "$target_arch")

    # Set profile via absolute symlink to host sandbox's portage tree.
    # The sysroot gets bind-mounted at /usr/$chost, so the symlink target
    # must resolve relative to the sandbox root, not the sysroot.
    # eselect profile can't be used because host ARCH != target ARCH.
    local profile_link="$sysroot_dir/etc/portage/make.profile"
    rm -rf "$profile_link"
    ln -s "/var/db/repos/gentoo/profiles/${profile}" "$profile_link"

    local host_make_conf="$sysroot_dir/etc/portage/make.conf"
    local cpu_count
    cpu_count=$(nproc 2>/dev/null || echo 4)
    local p=$((cpu_count / 2 + 1))
    local q="$cpu_count"
    # Cross-emerge requires CBUILD (host), CHOST (target), and ROOT
    set_make_conf_var "$host_make_conf" "CBUILD" "$(uname -m)-pc-linux-gnu"
    set_make_conf_var "$host_make_conf" "CHOST" "${chost}"
    set_make_conf_var "$host_make_conf" "ROOT" "${crossdev_root}/"
    set_make_conf_var "$host_make_conf" "CFLAGS" "${cflags}"
    set_make_conf_var "$host_make_conf" "CXXFLAGS" "${cflags}"
    set_make_conf_var "$host_make_conf" "MAKEOPTS" "-j${p} --load-average ${q}"
    set_make_conf_var "$host_make_conf" "EMERGE_DEFAULT_OPTS" "--jobs=${p} --load-average ${q}"
    set_make_conf_var "$host_make_conf" "FEATURES" "parallel-install -merge-wait"
    set_make_conf_var "$host_make_conf" "PKGDIR" "${crossdev_root}/packages"

    local llvm_target
    llvm_target=$(llvm_arch "$target_arch")
    if [[ -n "$llvm_target" ]]; then
        set_make_conf_var "$host_make_conf" "LLVM_TARGETS" "${llvm_target}"
    fi

    if [[ -n "$mirror" ]]; then
        set_make_conf_var "$host_make_conf" "GENTOO_MIRRORS" "${mirror}"
    fi

    # Portage env overrides (same as crossdev setup)
    mkdir -p "$sysroot_dir/etc/portage/env"
    mkdir -p "$sysroot_dir/etc/portage/package.env"
    mkdir -p "$sysroot_dir/etc/portage/package.use"
    mkdir -p "$sysroot_dir/etc/portage/package.accept_keywords"

    cat > "$sysroot_dir/etc/portage/env/plain.conf" << 'EOF'
CFLAGS="-O3 -pipe"
CXXFLAGS="-O3 -pipe"
EOF
    echo "dev-lang/rust plain.conf" > "$sysroot_dir/etc/portage/package.env/rust"

    cat > "$sysroot_dir/etc/portage/package.use/busybox" << 'EOF'
>=virtual/libcrypt-2-r1 static-libs
>=sys-libs/libxcrypt-4.4.36-r3 static-libs
>=sys-apps/busybox-1.36.1-r3 -pam static
EOF
    echo "llvm-core/clang -extra" > "$sysroot_dir/etc/portage/package.use/clang"
    echo "dev-lang/rust rustfmt -system-llvm" > "$sysroot_dir/etc/portage/package.use/rust"
    echo "dev-vcs/git -iconv" > "$sysroot_dir/etc/portage/package.use/git"
    echo "<sys-devel/gcc-16.0.9999:16 **" > "$sysroot_dir/etc/portage/package.accept_keywords/gcc"

    # Cross-compile cache: configure tests that try to run target binaries
    # will fail on the build host. Pre-seed known-good answers.
    cat > "$sysroot_dir/etc/portage/env/cross-cache.conf" << 'ENVEOF'
# Force cross-compilation mode. Without this, binfmt_misc may trick
# autoconf into thinking it can run target binaries (then failing).
cross_compiling=yes
ac_cv_func_eventfd=yes
ac_cv_func_epoll_create1=yes
ac_cv_func_malloc_0_nonnull=yes
ac_cv_func_realloc_0_nonnull=yes
mhd_cv_eventfd_usable=yes
ENVEOF
    echo "*/* cross-cache.conf" > "$sysroot_dir/etc/portage/package.env/cross-cache"

    # Step 3: Rebuild glibc with board CFLAGS
    # Only glibc needs board-specific CFLAGS in the sysroot (ABI definition,
    # vector-optimized memcpy/memset). Other libraries at generic CFLAGS are
    # fine for link-time symbol resolution. linux-headers included for
    # correctness (header-only, cheap).
    echo "==> Rebuilding glibc with CFLAGS: $cflags"
    run_with_sysroot "$sandbox_dir" "$sysroot_dir" "$chost" \
        "${chost}-emerge -1 -b -k sys-libs/glibc sys-kernel/linux-headers"

    # Record metadata
    echo "$cflags" > "$sysroot_dir/.cflags"
    echo "$target_arch" > "$sysroot_dir/.arch"
    echo "$(date -u +%Y%m%dT%H%M%SZ)" > "$sysroot_dir/.created"

    echo "Sysroot '$name' created at $sysroot_dir"
}

# run_with_sysroot <sandbox_dir> <sysroot_dir> <chost> <command>
# Runs a command in the sandbox with the sysroot bind-mounted at /usr/$chost.
run_with_sysroot() {
    local sandbox_dir="$1"
    local sysroot_dir="$2"
    local chost="$3"
    shift 3
    _hakoniwa_run "$sandbox_dir" "$*" \
      -B "$sysroot_dir":/usr/"$chost"
}

# list_sysroots: print available sysroots with their CFLAGS
list_sysroots() {
    if [[ ! -d "$SYSROOTS_DIR" ]]; then
        echo "No sysroots found."
        return
    fi
    for d in "$SYSROOTS_DIR"/*/; do
        [[ -d "$d" ]] || continue
        local name
        name=$(basename "$d")
        local cflags="(unknown)"
        [[ -f "$d/.cflags" ]] && cflags=$(cat "$d/.cflags")
        local arch="(unknown)"
        [[ -f "$d/.arch" ]] && arch=$(cat "$d/.arch")
        printf "%-25s %-10s %s\n" "$name" "$arch" "$cflags"
    done
}
