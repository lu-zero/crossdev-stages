# crossdev-stages
Build Gentoo stages leveraging crossdev

## Status

- [x] Build and assemble packages to a stage1 [catalyst](https://wiki.gentoo.org/wiki/Catalyst) can leverage
- [x] Update a compatible stage3 image
- [x] Build opensbi + u-boot images and linux kernel + modules
- [x] Assemble bootable images
- [x] Per-CFLAGS sysroot isolation (glibc-only rebuild)
- [x] Rust CLI (`crossdev-stages`) using [hakoniwa](https://github.com/souk4711/hakoniwa) for sandboxing

## Platforms
- riscv64 (BPI-F3, Milk-V Jupiter, DC Roma II, OrangePi RV2, K230, Blackhole P100/P150)

## Rust CLI

```
crossdev-stages [OPTIONS] <COMMAND>

Commands:
  sandbox   Manage host build sandboxes
  target    Manage target sysroots
  sysroot   Manage cross-compilation sysroots
  image     Build board images
  stages    List or download Gentoo stage3 tarballs
  cleanup   Clean up stale builds, orphan sysroots, and old stages

Options:
  --project-dir <DIR>     Project root (where boards/ lives) [default: .]
  --mirror <URL>          Gentoo mirror URL
  --sysroot-override <N>  Override board's SYSROOT
  --dry-run               Show what would be done
```

### Quick start

```sh
# Set up host sandbox
crossdev-stages sandbox setup --arch riscv64
crossdev-stages sandbox prepare
crossdev-stages sandbox crossdev --arch riscv64 --board k1

# Create per-CFLAGS sysroot (only rebuilds glibc)
crossdev-stages sysroot create rv64gcv_zvl256b k1

# Build an image
crossdev-stages image build --board k1

# Clean up stale builds and old stages
crossdev-stages cleanup --dry-run
crossdev-stages cleanup
```

### Sysroot isolation

Boards with different CFLAGS get separate sysroots. Only glibc is rebuilt
with board-specific flags (the only ABI-critical package). Other libraries
in the sysroot are generic -- the target rootfs gets its own copies via
cross-emerge.

Boards that share the same CFLAGS share a sysroot and its binary package
cache (`PKGDIR`). For example, K1 and KY-X1 both use `rv64gcv_zvl256b`.

## Dependencies
``` sh
# Needed to build all the stages
emerge crossdev merge-usr git
# Needed to build the bootloader and kernel
emerge u-boot-tools dtc dracut busybox
# Needed to investigate the image
emerge bubblewrap
# Needed to assemble the whole image
emerge genimage xz-utils
```
### For the newcomers
**crossdev** requires a minimum amount of [setup](https://wiki.gentoo.org/wiki/Crossdev#eselect_creation):
```
emerge app-eselect/eselect-repository
eselect repository create crossdev
```

## Board configuration

Each board lives in `boards/<name>/` with:
- `board.conf` -- arch, CFLAGS, repos, boot config
- `board.sh` (optional) -- override functions for checkout/assemble steps
- `genimage.cfg` -- disk image layout

### board.conf variables

| Variable | Required | Description |
|---|---|---|
| `BOARD_ARCH` | yes | Target architecture (e.g. `riscv64`) |
| `BOARD_CFLAGS` | no | Board-specific CFLAGS (default: arch default) |
| `BOARD_LDFLAGS` | no | Board-specific LDFLAGS (default: profile default) |
| `BOARD_RUSTFLAGS` | no | Board-specific RUSTFLAGS (default: none) |
| `SYSROOT` | yes | Sysroot name (boards with same value share a sysroot) |
| `BUILD_STEPS` | no | Build pipeline steps (default: deps checkout bootloader kernel assemble pack) |

## Limitations

- Some packages are cross-compilation unfriendly and rely on runtime checks (e.g. git iconv checks)
