# Configuration

crossdev-stages is configured through a cascade of text files. Everything is
**optional**: embedded defaults shipped with the binary cover every path, and
the project / board / CLI layers only override what you care about.

The design borrows Portage's own conventions — directory-based drop-ins,
bash-style `KEY="value"` `.conf` files, line-per-entry `.txt` lists. No new
formats to learn.

---

## Directory layout

```
crossdev-stages/                        <- crate root (embedded at compile time)
├── config/                             Rust reads + interprets
│   ├── arch/
│   │   ├── x86_64.conf                 arch defaults (cflags, profile, ...)
│   │   ├── aarch64.conf
│   │   └── riscv64.conf
│   ├── build.conf                      host-side build policy (GCC_SLOT)
│   ├── host-packages.txt               emerged in sandbox prepare (-b -k)
│   ├── host-bin-packages.txt           emerged binary-only (-G)
│   ├── crossdev-extra-packages.txt     crossdev --ex-pkg additions
│   └── portage/default/                copied verbatim into /etc/portage/
│       ├── env/plain.conf
│       ├── package.env/rust
│       └── package.use/{busybox,clang,git,rust}
│
<project_root>/
└── boards/
    └── <name>/                         per-board customization (all optional)
        ├── board.conf                  required: arch, kernel repo/tag, ...
        ├── genimage.cfg                disk image layout
        ├── target-packages.txt         cross-emerged into target
        ├── sandbox-packages.txt        extra host-side packages
        ├── make.conf                   appended to target /etc/portage/make.conf
        ├── patches/
        │   ├── kernel/*.patch
        │   ├── u-boot/*.patch
        │   ├── opensbi/*.patch
        │   └── firmware/*.patch
        ├── kernel-config/*.config      merged onto base defconfig
        ├── pre-<step>.sh               runs before Rust default
        ├── post-<step>.sh              runs after Rust default
        └── override-<step>.sh          replaces Rust default entirely
```

---

## How the layers compose

### `/etc/portage/` (host sandbox, cross-sysroot, and target)

```
1. config/portage/default/*             embedded fragments (env/, package.use/, ...)
2. MakeConf::write()                    make.conf managed vars (MAKEOPTS, FEATURES,
                                        CHOST, CFLAGS, ACCEPT_KEYWORDS, PORT_LOGDIR,
                                        GENTOO_MIRRORS, PORTAGE_BINHOST) — updated
                                        in place, preserving stage3 defaults
3. package.accept_keywords/gcc          written dynamically from GCC_SLOT
4. --portage-overlay <DIR>              CLI flag, copied on top (optional)
5. boards/<name>/make.conf              target only, marker-wrapped append
```

Later layers win. Layer 4 is host + cross-sysroot + target; layer 5 is target only.

make.conf is **not** a fragment — Rust edits it in place so stage3's
catalyst-written lines (comments, `COMMON_FLAGS`, `LC_MESSAGES`, …) survive.
The baseline `FEATURES` value comes from `config/build.conf`.

### Build pipeline (`image build`)

```
for step in [deps, checkout, bootloader, kernel, assemble, pack]:
    if boards/<name>/override-<step>.sh   -> run it instead
    else:
        if boards/<name>/pre-<step>.sh    -> run
        run Rust default_<step>()
        if boards/<name>/post-<step>.sh   -> run
```

Inside `default_checkout`: `patches/<component>/*.patch` applied after each
component clone (alphabetical).

Inside `default_kernel`: after `make <defconfig>`, if
`kernel-config/*.config` exists, run `scripts/kconfig/merge_config.sh -m`
then `make olddefconfig` before the kernel build.

---

## Adding a new board

1. `mkdir boards/<name>/`
2. Write `boards/<name>/board.conf`:
   ```
   BOARD_ARCH="riscv64"
   BOARD_CFLAGS="-O3 -march=rv64gc -pipe"
   CROSS_COMPILE="riscv64-unknown-linux-gnu-"
   KERNEL_ARCH="riscv"

   KERNEL_REPO="https://github.com/..."
   KERNEL_TAG="v6.14"
   KERNEL_DEFCONFIG="my_board_defconfig"
   KERNEL_NAME="Image"

   # Optional (bootloader, firmware, ...)
   OPENSBI_REPO="https://github.com/riscv-software-src/opensbi.git"
   OPENSBI_TAG="v1.7"
   OPENSBI_PLATFORM="generic"

   U_BOOT_REPO="https://source.denx.de/u-boot/u-boot.git"
   U_BOOT_TAG="v2025.07"
   U_BOOT_DEFCONFIG="my_board_defconfig"

   BOOT_HOSTNAME="gentoo-myboard"
   BOOT_CONSOLE="ttyS0,115200"
   BOOT_SERIAL_TTY="ttyS0"
   BOOT_SERIAL_BAUD="115200"
   ```
3. Write `boards/<name>/genimage.cfg` (see existing boards).
4. (Optional) Drop any of: `target-packages.txt`, `patches/<component>/*.patch`,
   `kernel-config/*.config`, `make.conf`, `pre-/post-/override-*.sh`.
5. Dry-run: `crossdev-stages image build --board <name> --dry-run`.
6. Real build: `crossdev-stages image build --board <name>`.

Nothing in Rust needs to change.

---

## Adding a new arch

Supported arches live in `crossdev-stages/config/arch/`. Adding a new one:

1. `crossdev-stages/config/arch/<arch>.conf`:
   ```
   GENTOO_ARCH="..."        # Gentoo keyword (amd64, arm64, riscv, ...)
   PROFILE="..."            # default/linux/.../23.0/...
   KERNEL_ARCH="..."        # Linux ARCH= value
   CFLAGS="..."             # default CFLAGS
   LLVM_TARGET="..."        # LLVM_TARGETS (X86, AArch64, RISCV, ...)
   STAGE_VARIANT="..."      # stage3 variant name on mirrors
   ```
2. In `crossdev-stages/src/stage.rs`, add one line to `ARCH_CONFIGS`:
   ```rust
   m.insert("<arch>", parse_arch_conf(include_str!("../config/arch/<arch>.conf")));
   ```
3. Rebuild.

---

## Per-file reference

### `config/build.conf`

Host-side build policy. All keys optional with sensible defaults.

```
GCC_SLOT="16"                                    # sys-devel/gcc:N slot
FEATURES_BASE="parallel-install -merge-wait"     # baseline FEATURES for make.conf
```

Changing `GCC_SLOT` affects: emerged gcc version, `gcc-config` selection,
`package.accept_keywords/gcc` unmask pattern (all derived from this value).

`FEATURES_BASE` is extended with `getbinpkg` automatically when `--binhost`
is set.

### `config/arch/<arch>.conf`

Per-arch defaults. See "Adding a new arch" above for the key list.

### `config/host-packages.txt`, `host-bin-packages.txt`, `crossdev-extra-packages.txt`

Line-per-atom package lists. `#` comments and blank lines ignored.

`host-bin-packages.txt` is installed with `emerge -G` (binary-only, fast).
`host-packages.txt` is installed with `emerge -b -k` (build if needed,
binpkg if available). `crossdev-extra-packages.txt` entries are passed to
`crossdev` as `--ex-pkg <atom>`.

### `config/portage/default/*`

Everything here is copied verbatim into the target `/etc/portage/`. Standard
Portage drop-in layout:

- `env/<name>.conf` — bash env overrides, referenced by `package.env/*`.
- `package.env/<name>` — `category/pkg env-file` mappings.
- `package.use/<name>` — `category/pkg use-flags` lines.

Note: `make.conf` is NOT here. It's edited in place by `MakeConf::write` so
stage3's catalyst-written content (comments, `COMMON_FLAGS`, `LC_MESSAGES`)
is preserved. The `FEATURES` value is policy, controlled by `FEATURES_BASE`
in `config/build.conf`.

To ship an additional default fragment: drop a file here, then add one
`include_str!` entry to `DEFAULT_FRAGMENTS` in `src/portage.rs`.

### `boards/<name>/board.conf`

See "Adding a new board" for the common variables. Full list is in
`crossdev-stages/src/board.rs`.

Bash arrays allowed:
```
BOOT_SERVICES=("sshd:default" "chronyd:default")
WORKAROUND_PKGS=("dev-lang/rust")
WORKAROUND_CFLAGS=("-O2 -pipe")
BUILD_STEPS=("deps" "checkout" "kernel" "bootloader" "assemble" "pack")
```

### `boards/<name>/target-packages.txt`, `sandbox-packages.txt`

Line-per-atom. `target-packages.txt` entries are cross-emerged into the
target rootfs at the `deps` step. `sandbox-packages.txt` entries are
installed on the host sandbox (useful for board-specific build tools).

### `boards/<name>/make.conf`

Appended to the target `/etc/portage/make.conf` wrapped in
`# [crossdev-stages: begin|end boards/<name>/make.conf]` markers. Switching
boards strips the previous block and inserts the current board's.

Use for per-board-image Portage knobs:
```
VIDEO_CARDS="rockchip panfrost"
USE="-X -wayland"
ACCEPT_LICENSE="@FREE linux-firmware"
```

Avoid variables managed by [`MakeConf`](../crossdev-stages/src/portage.rs)
(MAKEOPTS, CHOST, CFLAGS, CXXFLAGS, FEATURES, ACCEPT_KEYWORDS, PORT_LOGDIR,
LLVM_TARGETS, GENTOO_MIRRORS, PORTAGE_BINHOST) — those get overwritten on
stage1 re-runs.

### `boards/<name>/patches/<component>/*.patch`

Applied with `patch -p1` in **alphabetical order** after each component's
git clone (at the `checkout` step). Numeric prefixes (`0001-`, `0002-`)
give explicit ordering.

Components: `kernel`, `u-boot`, `opensbi`, `firmware`.

`.mbox` and `git am` are **not** supported — split mbox series into
individual `.patch` files first.

### `boards/<name>/kernel-config/*.config`

Kconfig fragments in alphabetical order. Applied via the kernel's own
`scripts/kconfig/merge_config.sh -m .config <fragments>` followed by
`make olddefconfig`. Runs after `make <KERNEL_DEFCONFIG>`, before the
kernel compile.

Example:
```
# boards/odroid-m2/kernel-config/10-rkvdec.config
CONFIG_VIDEO_ROCKCHIP_VDEC=m
```

### `boards/<name>/pre-/post-/override-<step>.sh`

Bash hooks per build step. `board.conf` is sourced automatically, so all
variables are available. Scripts run inside the sandbox container;
`$CROSS_COMPILE`, `$KERNEL_ARCH`, etc. are exported.

### `--portage-overlay <DIR>`

CLI flag (global). Directory with the same layout as
`config/portage/default/` — any file present is copied on top of the
embedded defaults into both the host sandbox and the cross-sysroot
`/etc/portage/`. Good for one-off experimentation.

---

## Gotchas

- **Kernel patch reapplication**: the `checkout` step is idempotent via
  a `.done/sources` marker. Adding a new patch after a build finished the
  checkout step does **not** re-apply — delete the marker manually:
  ```
  rm ~/.cache/crossdev-stages/builds/<board>-*/.done/sources
  ```

- **Target portage fragments in the image**: defaults at
  `config/portage/default/*` end up in the final rootfs `/etc/portage/`.
  Users of the image see our opinions there. Delete those files in a
  `post-assemble.sh` if you want pristine Gentoo.

- **`~ARCH` mandatory**: our toolchain (gcc prerelease, rust, clang-crossdev-
  wrappers) is testing-only on all supported arches. Switching to stable
  is not supported — edit `src/portage.rs:MakeConf::write` if you need it.
