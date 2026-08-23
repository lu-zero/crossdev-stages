# crossdev-stages
Rootless cross-compilation of Gentoo stages using crossdev and hakoniwa

## Status

- [x] Build and assemble packages to a stage1 [catalyst](https://wiki.gentoo.org/wiki/Catalyst) can leverage
- [x] Update a compatible stage3 image
- [x] Build opensbi + u-boot images and linux kernel + modules
- [x] Assemble bootable images
- [x] Content-addressed crossdev prefix store, keyed by `(chost, CFLAGS-hash, gcc)`
- [x] Shared binpkg cache, keyed by `(chost, CFLAGS-hash)`
- [x] `build.lock.toml` per image with pinned source commits + CFLAGS
- [x] `crossdev-stages update` to compare lock vs upstream HEAD
- [x] Rust CLI using [hakoniwa](https://github.com/souk4711/hakoniwa) for sandboxing
- [x] Modular bootloader (opensbi, u-boot, grub, syslinux, tfa, rkbin)
- [x] File-convention hooks (pre/post/override scripts per build step)
- [x] Git source cache (bare repo references)

## Boards

| Board | Arch | Kernel | Boot chain | CFLAGS | Status |
|---|---|---|---|---|---|
| k1 | riscv64 | spacemit 6.6 | OpenSBI + U-Boot | `-O3 -march=rv64gcv_zvl256b` | stable |
| k1-upstream | riscv64 | spacemit k3 | OpenSBI + U-Boot | `-O3 -march=rv64gcv_zvl256b` | testing |
| k3 | riscv64 | spacemit 6.18 | OpenSBI + U-Boot | `-O3 -march=rva23u64` | stable |
| k230 | riscv64 | canaan (hdmi) | OpenSBI (payload) + U-Boot | `-O3 -march=rv64gcv_zvl128b` | stable |
| ky-x1 | riscv64 | spacemit 6.6 | OpenSBI + U-Boot | `-O3 -march=rv64gcv_zvl256b` | stable |
| blackhole | riscv64 | tenstorrent | OpenSBI (jump, PCIe BAR) | `-O3 -march=rv64gcv_zvl512b` | stable |
| odroid-m1 | aarch64 | mainline v7.0 | TFA + U-Boot + rkbin | `-O3 -mcpu=cortex-a55` | testing |
| odroid-m1s | aarch64 | mainline v7.0 | TFA + U-Boot + rkbin | `-O3 -mcpu=cortex-a55` | testing |
| odroid-m2 | aarch64 | mainline v7.0 | TFA + U-Boot + rkbin | `-O3 -mcpu=cortex-a76.cortex-a55` | testing |
| pentium-mmx | i586 | mainline v6.12 | BIOS (no firmware) | `-O2 -march=pentium-mmx` | testing |

## CLI

```
crossdev-stages [OPTIONS] <COMMAND>

Commands:
  sandbox   Manage host build sandboxes
  target    Manage cross-compiled target stages
  image     Build board images
  stages    List or download Gentoo stage3 tarballs
  board     Manage and inspect boards
  maint     Maintenance: clean, logs, diagnostics
  status    Show overview of sandboxes, targets, builds, and boards
  store     Manage the content-addressed crossdev prefix store
  update    Compare a board's build.lock.toml against upstream HEAD

Options:
  --project-dir <DIR>  Project root (where boards/ lives) [default: .]
  --mirror <URL>       Gentoo mirror URL
  --binhost <URL>      Binary package host URL
  --dry-run            Show what would be done
```

### Quick start

```sh
# List available boards
crossdev-stages board list

# Inspect a board configuration
crossdev-stages board info --board <BOARD>

# Set up host sandbox
crossdev-stages sandbox setup
crossdev-stages sandbox prepare
crossdev-stages sandbox crossdev --arch <ARCH> --board <BOARD>

# Create target stage from a stage3 seed
crossdev-stages target setup --arch <ARCH>
crossdev-stages target stage1
crossdev-stages target update

# Build an image
crossdev-stages image build --board <BOARD>

# Inspect builds, sandboxes, and the toolchain store
crossdev-stages status

# See what would change on a fresh build (read-only)
crossdev-stages update --board <BOARD>
crossdev-stages update --all

# Export the image
crossdev-stages image export --board <BOARD> -o /tmp/

# Export a full flash bundle (all boot blobs + images), optionally as .tar.xz
crossdev-stages image export --board <BOARD> --all --tar -o /tmp/

# List toolchain store entries; delete ones no current board uses
crossdev-stages store list
crossdev-stages store gc --force

# Clean up stale builds and old stage3 tarballs
crossdev-stages maint clean

# Wipe whole categories (replaces sudo rm -rf ~/.cache/crossdev-stages)
crossdev-stages maint clean --sandboxes --targets
crossdev-stages maint clean --all
```

`--all` copies every build artifact into a `<BOARD>-flash-bundle/` directory.
If `boards/<BOARD>/bundle.list` exists it acts as a whitelist: one build-dir
relative path per line, `#` comments ignored. A missing entry fails the
export unless prefixed with `optional:`, and the token `@image` resolves to
the packed image filename (which carries a UTC timestamp) from the build's
`.image` marker. Without a `bundle.list` the whole build dir is copied,
skipping the `gen/`, `linux/`, `tmp/`, and `firmware/` source trees.

### Image manifest

Every `image build` emits an `<image>.manifest.json` next to the
compressed disk image, listing board, full-image sha256, and the
partition table parsed from `genimage.cfg`:

```json
{
  "board": "k230",
  "image": "gentoo-linux-k230_dev-sdcard-20260430T165016Z.img",
  "sha256": "d1632bbf...",
  "partitions": [
    { "name": "uboot_spl_1", "offset": "1024K", "size": "512K",
      "image": "u-boot/fn_u-boot-spl.bin", "sha256": "273f5662..." },
    ...
  ]
}
```

Useful for verifying integrity and writing individual partitions to
eMMC/SPI flash at known offsets without re-parsing `genimage.cfg`.

### Cache layout

Everything lives under `~/.cache/crossdev-stages/`:

| Path | Contents |
|---|---|
| `stages/` | downloaded stage3 tarballs |
| `sources/<repo>.git/` | bare git mirrors used as `--reference` for fast clones |
| `sandboxes/<name>/` | host stage3 unpacks; `.overlay-upper-*/` dirs hold per-toolchain overlay writes |
| `targets/<name>/` | cross-compiled target rootfs |
| `store/<chost>/<cflags-hash>-gcc<ver>/` | immutable crossdev prefix; built once per `(chost, canonical CFLAGS, gcc version)` and overlay-mounted at `/usr/<chost>/` for every build that needs it |
| `binpkgs/<chost>/<cflags-hash>/` | shared `PKGDIR` for cross-compiled packages so different boards with compatible CFLAGS reuse builds |
| `builds/<board>/<timestamp>/` | per-image build output, including `build.lock.toml` |
| `logs/` | build step logs |

CFLAGS are canonicalized via [sokgi](https://github.com/OctopusET/sokgi)
before hashing, so semantically equivalent flag strings (different token
order, last-wins overrides) share a store entry.  `store list` shows what
is present; `store gc` removes entries (and binpkg caches) no current
board resolves to.

## Requirements

Linux 5.11 or newer with unprivileged user-namespace overlayfs enabled
(`unprivileged_userns_clone=1` on Debian/Ubuntu kernels; default on most
others).  The store is mounted as overlayfs inside a hakoniwa user
namespace; macOS/BSD are not supported.

## Dependencies
```sh
emerge crossdev merge-usr git
emerge u-boot-tools dtc dracut busybox
emerge genimage xz-utils
```

**crossdev** requires a minimum amount of [setup](https://wiki.gentoo.org/wiki/Crossdev#eselect_creation):
```
emerge app-eselect/eselect-repository
eselect repository create crossdev
```

## Board configuration

Each board lives in `boards/<name>/` with:
- `board.conf` -- variables read by Rust and bash scripts
- `genimage.cfg` -- disk image layout
- `sandbox-packages.txt` -- extra host packages for the sandbox (optional)
- `sandbox-packages.use` -- USE flags for those packages (optional)
- `target-packages.txt` -- extra packages cross-emerged into the image (optional)
- `pre-{step}.sh` -- runs before Rust default (optional)
- `post-{step}.sh` -- runs after Rust default (optional)
- `override-{step}.sh` -- replaces Rust default entirely (optional)

Steps: `deps`, `checkout`, `bootloader`, `kernel`, `assemble`, `pack`

Package lists in `defaults/` apply to every sandbox
(`defaults/sandbox-packages.txt`) and every image
(`defaults/target-packages.txt`).  The effective target set is
`defaults/target-packages.txt` UNION `boards/<name>/target-packages.txt`
MINUS the board's `-atom` lines (e.g. `-app-misc/fastfetch` drops a
default) -- every part is a plain file.  Subtracting an atom not in the
set warns and does nothing; `-atom` lines in the defaults file itself
are an error.  Heavy extras (mold, go, cmake, rust, iw, wpa_supplicant)
stay out of the defaults -- boards opt in via their own list.
List lines are `atom [keywords]` -- a keyword override (e.g.
`sys-boot/syslinux **`) lands in `etc/portage/package.accept_keywords/`.

### Build step execution

```
1. override-{step}.sh exists?  -> run it, done
2. pre-{step}.sh exists?       -> run it
3. Rust module default
4. post-{step}.sh exists?      -> run it
```

### Bootloader pipeline

The `bootloader` step runs an ordered list of stages declared in the
`BOOT_PIPELINE` array. Valid stage names: `opensbi`, `uboot`, `grub`,
`syslinux`, `tfa`, `rkbin`, `amlogic-fip` (validated when board.conf is
loaded). If the key is omitted the default
`("opensbi" "uboot" "syslinux" "grub")` applies; each stage is a no-op
unless its board.conf keys are set, so the default covers both the
RISC-V vendor SDK pattern and the x86 BIOS pattern. An explicitly empty
array `()` runs no stages (all-prebuilt firmware). Stages pass data
forward via env exports prepended to later stages' build commands, e.g.
`tfa` exports `BL31=` and `rkbin` exports `ROCKCHIP_TPL=`, both consumed
by `uboot`.

### board.conf variables

| Variable | Required | Description |
|---|---|---|
| `BOARD_NAME` | yes | Board identifier (matches directory name) |
| `INCLUDE` | no | Shared config chunks to read before this file |
| `BOARD_ARCH` | yes | Target architecture (`riscv64`, `aarch64`, `i586`, `i686`) |
| `CROSS_COMPILE` | yes | Toolchain prefix (e.g. `riscv64-unknown-linux-gnu-`) |
| `KERNEL_REPO` | yes | Kernel source repository URL |
| `KERNEL_DEFCONFIG` | yes | Kernel defconfig name |
| `KERNEL_CONFIG_FRAGMENTS` | no | Config fragments to apply after the defconfig |
| `CHOST` | no | Override derived CHOST triple (default: auto from arch) |
| `BOARD_CFLAGS` | no | Board-specific CFLAGS (default: arch default) |
| `KERNEL_TAG` | no | Kernel git ref (default: top-level `TAG`) |
| `KERNEL_ARCH` | no | Linux `ARCH=` value (default: auto from `BOARD_ARCH`) |
| `BUILD_STEPS` | no | Build pipeline steps (default: deps checkout bootloader kernel assemble pack) |
| `BOOT_PIPELINE` | no | Ordered bootloader stages (default: `("opensbi" "uboot" "syslinux" "grub")`; `()` = none) |
| `BOOT_EXTLINUX` | no | `true` makes `assemble` write `/extlinux/extlinux.conf` |
| `BOOT_APPEND` | no | Kernel arguments added to that entry |
| `BOOT_DTB_NAME` | no | DTB to boot, when `BOARD_DTB_GLOB` matches more than one |
| `OPENSBI_FW_TYPE` | no | OpenSBI firmware type: `dynamic` (default), `jump`, `payload` |
| `OPENSBI_MAKE_FLAGS` | no | Extra opensbi make arguments |
| `U_BOOT_MAKE_FLAGS` | no | Extra u-boot make arguments |
| `GRUB_PLATFORMS` | no | GRUB platform (e.g. `pc`); enables the `grub` stage (grub-mkimage) |
| `GRUB_MODULES` | no | GRUB modules embedded in core.img (default: BIOS boot set) |
| `SYSLINUX_REPO` / `SYSLINUX_TAG` | no | SYSLINUX source repo + tag; enables the `syslinux` stage |
| `TFA_REPO` / `TFA_TAG` / `TFA_PLAT` | no | ARM Trusted Firmware-A (BL31) repo, tag (default `master`), platform |
| `RKBIN_REPO` / `RKBIN_TAG` / `RKBIN_DDR` | no | Rockchip blob repo, tag (default `master`), DDR-init blob glob |
| `FIP_REPO` / `FIP_TAG` | no | Amlogic boot-FIP packaging repo, tag (default `master`) |
| `FIRMWARE_TAG` | no | Tag for the firmware overlay repo (default: `TAG`) |
| `COMPRESSION` | no | Image compression: `xz` (default), `gz`, `none` |
| `TAGS` | no | Free-form labels (bash array, e.g. `TAGS=("testing" "wip")`) -- shown in `board list` and `status` |
| `DESCRIPTION` | no | Free-form note shown in `board info` |

### Shared board config

`INCLUDE` names files under `boards/include/`, read in the order listed before
the board's own `board.conf`:

```
INCLUDE="rk35xx"
```

The last assignment of a key wins, so a later include beats an earlier one and
the board beats all of them. A board therefore states only what is actually
its own, with no guard around any shared default.

Includes compose: a board can pull in an SoC chunk and a role chunk at once.
An include cannot itself include, which keeps "where did this value come from"
answerable. Hooks source the same files in the same order, so a shell script
sees exactly what `board info` reports.

### Kernel config fragments

`KERNEL_CONFIG_FRAGMENTS` names files, in the order they should apply, looked
up as `boards/<board>/kernel-config/<name>` and then
`defaults/kernel-config/<name>`. Each holds literal `.config` lines:

```
# CONFIG_RISCV_ISA_V is not set
CONFIG_DRM_ACCEL=y
CONFIG_DRM_ACCEL_ROCKET=m
```

They are appended after the defconfig, `olddefconfig` runs, and then every
line is checked against the result. `olddefconfig` drops a symbol whose
dependencies are unmet and can return a module where a builtin was asked for,
both silently, so the build fails rather than shipping a kernel that is
missing what the board said it needed. A `# CONFIG_X is not set` line is
checked in the other direction: the build fails if X came back on.

### Extlinux boot config

A board whose bootloader reads `/extlinux/extlinux.conf` sets
`BOOT_EXTLINUX="true"` and `assemble` writes the file:

```
DEFAULT gentoo
TIMEOUT 30
LABEL gentoo
    MENU LABEL Gentoo Linux
    LINUX /<BOOT_KERNEL_NAME>
    FDT /<dtb>
    APPEND root=<BOOT_ROOT_DEV> rw rootwait rootfstype=ext4 console=<BOOT_CONSOLE> <BOOT_APPEND>
```

The DTB is the basename of `BOARD_DTB_GLOB`, which on most boards already names
exactly one file; a board that copies a whole directory of them sets
`BOOT_DTB_NAME`. A board needing a different shape -- several labels, a kernel
named by version -- writes its own file from `post-assemble.sh` instead.

### Partition identifiers

The `pack` step exports a set of identifiers derived from the board, so a board
can name its root filesystem without depending on which slot the card lands in.
Board scripts see them before `board.conf` is sourced, and genimage expands
them in `genimage.cfg` through `${VAR}`:

| Variable | Description |
|---|---|
| `BOOT_DISK_ID` | MBR disk signature, 8 hex digits, never zero |
| `BOOT_DISK_SIG` | The same value as `0x...`, for genimage's `disk-signature` |
| `BOOT_DISK_UUID` | GPT disk UUID, for genimage's `disk-uuid` |
| `BOOT_PART_UUID_1`..`_4` | GPT partition UUIDs, for `partition-uuid` |

The kernel understands only `PARTUUID=`, `PARTLABEL=` and `/dev/` paths without
an initramfs (`block/early-lookup.c`), and it spells an MBR PARTUUID as the disk
signature and the 1-based slot: `BOOT_ROOT_DEV="PARTUUID=${BOOT_DISK_ID}-02"`
for the second partition. GPT boards use `BOOT_PART_UUID_N` directly.

The values come from a hash of what decides the image -- board name, arch,
CHOST, kernel repo and tag, CFLAGS -- so two boards never collide and building
the same board twice produces the same disk. Editing a comment in `board.conf`
does not change them. genimage's own `disk-signature = random` cannot be used
here: `assemble` writes the boot config before `pack` creates the partition
table, so the value has to be known first.

## Limitations

- Some packages are cross-compilation unfriendly and rely on runtime checks (e.g. git iconv checks)
