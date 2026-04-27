# crossdev-stages
Rootless cross-compilation of Gentoo stages using crossdev and hakoniwa

## Status

- [x] Build and assemble packages to a stage1 [catalyst](https://wiki.gentoo.org/wiki/Catalyst) can leverage
- [x] Update a compatible stage3 image
- [x] Build opensbi + u-boot images and linux kernel + modules
- [x] Assemble bootable images
- [x] Content-addressed crossdev prefix store, keyed by `(chost, CFLAGS-hash)`
- [x] Shared binpkg cache, keyed the same way
- [x] `build.lock.toml` per image with pinned source commits + CFLAGS
- [x] `crossdev-stages update` to compare lock vs upstream HEAD
- [x] Rust CLI using [hakoniwa](https://github.com/souk4711/hakoniwa) for sandboxing
- [x] Modular bootloader (opensbi, u-boot, tfa, rkbin)
- [x] File-convention hooks (pre/post/override scripts per build step)
- [x] Git source cache (bare repo references)

## Platforms
- riscv64 (BPI-F3, Milk-V Jupiter, DC Roma II, OrangePi RV2, K230, Blackhole P100/P150)
- aarch64 (Odroid M2, Odroid C2, Odroid C4 -- testing)

## CLI

```
crossdev-stages [OPTIONS] <COMMAND>

Commands:
  sandbox   Manage host build sandboxes
  target    Manage cross-compiled target stages
  image     Build board images
  stages    List or download Gentoo stage3 tarballs
  board     Manage and inspect boards
  status    Show overview of sandboxes, targets, builds, and boards
  update    Compare a board's build.lock.toml against upstream HEAD

Options:
  --project-dir <DIR>  Project root (where boards/ lives) [default: .]
  --mirror <URL>       Gentoo mirror URL
  --binhost <URL>      Binary package host URL
  --dry-run            Show what would be done
```

### Quick start

```sh
# One-shot: ensures sandbox + crossdev prefix exist, then builds
crossdev-stages image build --board k1

# Inspect builds, sandboxes, and the toolchain store
crossdev-stages status

# See what would change on a fresh build (read-only)
crossdev-stages update --board k1
crossdev-stages update --all

# Export the image
crossdev-stages image export --board k1 -o /tmp/
```

### Cache layout

Everything lives under `~/.cache/crossdev-stages/`:

| Path | Contents |
|---|---|
| `stages/` | downloaded stage3 tarballs |
| `sources/<repo>.git/` | bare git mirrors used as `--reference` for fast clones |
| `sandboxes/<name>/` | host stage3 unpacks; `.upper-<chost>-<hash>/` directories hold per-toolchain overlay writes |
| `targets/<name>/` | cross-compiled target rootfs |
| `store/<chost>/<cflags-hash>/` | immutable crossdev prefix; built once per `(chost, canonical CFLAGS)` and overlay-mounted at `/usr/<chost>/` for every build that needs it |
| `binpkgs/<chost>/<cflags-hash>/` | shared `PKGDIR` for cross-compiled packages so different boards with compatible CFLAGS reuse builds |
| `builds/<board>/<timestamp>/` | per-image build output, including `build.lock.toml` |

CFLAGS are canonicalized via [sokgi](https://github.com/OctopusET/sokgi) before hashing, so semantically equivalent flag strings (different token order, last-wins overrides) share a store entry.

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
| `BUILD_STEPS` | no | Build pipeline steps (default: deps checkout bootloader kernel assemble pack) |
| `OPENSBI_FW_TYPE` | no | OpenSBI firmware type: `dynamic` (default), `jump`, `payload` |
| `OPENSBI_MAKE_FLAGS` | no | Extra opensbi make arguments |
| `U_BOOT_MAKE_FLAGS` | no | Extra u-boot make arguments |
| `COMPRESSION` | no | Image compression: `xz` (default), `gz`, `none` |

## Limitations

- Some packages are cross-compilation unfriendly and rely on runtime checks (e.g. git iconv checks)
