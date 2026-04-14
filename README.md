# crossdev-stages
Build Gentoo stages leveraging crossdev

## Status

- [x] Build and assemble packages to a stage1 [catalyst](https://wiki.gentoo.org/wiki/Catalyst) can leverage
- [x] Update a compatible stage3 image
- [x] Build opensbi + u-boot images and linux kernel + modules
- [x] Assemble bootable images
- [x] Per-CFLAGS sysroot isolation (glibc-only rebuild)
- [x] Rust CLI using [hakoniwa](https://github.com/souk4711/hakoniwa) for sandboxing
- [x] Modular bootloader (opensbi, u-boot, tfa, rkbin)
- [x] File-convention hooks (pre/post/override scripts per build step)
- [x] Git source cache (bare repo references)

## Platforms
- riscv64 (BPI-F3, Milk-V Jupiter, DC Roma II, OrangePi RV2, K230, Blackhole P100/P150)
- aarch64 (Odroid M2 -- testing)

## CLI

```
crossdev-stages [OPTIONS] <COMMAND>

Commands:
  sandbox   Manage host build sandboxes
  target    Manage target sysroots
  sysroot   Manage cross-compilation sysroots
  image     Build board images
  stages    List or download Gentoo stage3 tarballs
  cleanup   Clean up stale builds, orphan sysroots, and old stages
  status    Show overview of sandboxes, sysroots, builds, boards
  logs      Show build step timestamps
  export    Export build artifacts
  config    Show resolved board configuration
  doctor    Check environment for common issues

Options:
  --project-dir <DIR>     Project root (where boards/ lives) [default: .]
  --mirror <URL>          Gentoo mirror URL
  --sysroot-override <N>  Override board's SYSROOT
  --dry-run               Show what would be done
```

### Quick start

```sh
# Set up host sandbox
crossdev-stages sandbox setup
crossdev-stages sandbox prepare
crossdev-stages sandbox crossdev --arch riscv64 --board k1

# Create per-CFLAGS sysroot (only rebuilds glibc)
crossdev-stages sysroot create rv64gcv_zvl256b k1

# Build an image
crossdev-stages image build --board k1

# Check status
crossdev-stages status

# Export the image
crossdev-stages export k1 -o /tmp/

# Clean up stale builds and old stages
crossdev-stages cleanup
```

### Sysroot isolation

Boards with different CFLAGS get separate sysroots. Only glibc is rebuilt
with board-specific flags (the only ABI-critical package). Other libraries
in the sysroot are generic -- the target rootfs gets its own copies via
cross-emerge.

Boards that share the same CFLAGS share a sysroot and its binary package
cache (`PKGDIR`). For example, K1 and KY-X1 both use `rv64gcv_zvl256b`.

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
- `pre-{step}.sh` -- runs before Rust default (optional)
- `post-{step}.sh` -- runs after Rust default (optional)
- `override-{step}.sh` -- replaces Rust default entirely (optional)

Steps: `deps`, `checkout`, `bootloader`, `kernel`, `assemble`, `pack`

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
| `BOARD_ARCH` | yes | Target architecture (`riscv64`, `aarch64`) |
| `BOARD_CFLAGS` | no | Board-specific CFLAGS (default: arch default) |
| `SYSROOT` | yes | Sysroot name (boards with same value share a sysroot) |
| `BUILD_STEPS` | no | Build pipeline steps |
| `OPENSBI_FW_TYPE` | no | OpenSBI firmware type: `dynamic` (default), `jump`, `payload` |
| `OPENSBI_MAKE_FLAGS` | no | Extra opensbi make arguments |
| `U_BOOT_MAKE_FLAGS` | no | Extra u-boot make arguments |
| `COMPRESSION` | no | Image compression: `xz` (default), `gz`, `none` |

## Limitations

- Some packages are cross-compilation unfriendly and rely on runtime checks (e.g. git iconv checks)
