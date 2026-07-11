# crossdev-stages
Rootless cross-compilation of Gentoo stages using crossdev and hakoniwa

## Status

- [x] Build and assemble packages to a stage1 [catalyst](https://wiki.gentoo.org/wiki/Catalyst) can leverage
- [x] Update a compatible stage3 image
- [x] Build opensbi + u-boot images and linux kernel + modules
- [x] Assemble bootable images
- [x] Per-CFLAGS target stage isolation (glibc-only rebuild)
- [x] Rust CLI using [hakoniwa](https://github.com/souk4711/hakoniwa) for sandboxing
- [x] Modular bootloader (opensbi, u-boot, tfa, rkbin)
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

# Check status
crossdev-stages status

# Export the image
crossdev-stages image export --board <BOARD> -o /tmp/

# Clean up stale builds and old stage3 tarballs
crossdev-stages maint clean

# Wipe whole categories (replaces sudo rm -rf ~/.cache/crossdev-stages)
crossdev-stages maint clean --sandboxes --targets
crossdev-stages maint clean --all
```

### Source cache

Git repos are cached as bare repositories at `~/.cache/crossdev-stages/sources/`.
First clone fetches from upstream; subsequent builds use `--reference` for
near-instant checkout.

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
Portage config files under `defaults/portage/` are overlaid onto the
sandbox's `etc/portage/` during prepare.

### Build step execution

```
1. override-{step}.sh exists?  -> run it, done
2. pre-{step}.sh exists?       -> run it
3. Rust module default
4. post-{step}.sh exists?      -> run it
```

### board.conf variables

| Variable | Required | Description |
|---|---|---|
| `BOARD_NAME` | yes | Board identifier (matches directory name) |
| `BOARD_ARCH` | yes | Target architecture (`riscv64`, `aarch64`, `i586`, `i686`) |
| `CROSS_COMPILE` | yes | Toolchain prefix (e.g. `riscv64-unknown-linux-gnu-`) |
| `KERNEL_REPO` | yes | Kernel source repository URL |
| `KERNEL_DEFCONFIG` | yes | Kernel defconfig name |
| `CHOST` | no | Override derived CHOST triple (default: auto from arch) |
| `BOARD_CFLAGS` | no | Board-specific CFLAGS (default: arch default) |
| `KERNEL_TAG` | no | Kernel git ref (default: top-level `TAG`) |
| `KERNEL_ARCH` | no | Linux `ARCH=` value (default: auto from `BOARD_ARCH`) |
| `BUILD_STEPS` | no | Build pipeline steps (default: deps checkout bootloader kernel assemble pack) |
| `OPENSBI_FW_TYPE` | no | OpenSBI firmware type: `dynamic` (default), `jump`, `payload` |
| `OPENSBI_MAKE_FLAGS` | no | Extra opensbi make arguments |
| `U_BOOT_MAKE_FLAGS` | no | Extra u-boot make arguments |
| `COMPRESSION` | no | Image compression: `xz` (default), `gz`, `none` |
| `TESTING` | no | Mark board as testing (`true`/`false`) |

## Limitations

- Some packages are cross-compilation unfriendly and rely on runtime checks (e.g. git iconv checks)
