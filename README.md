# crossdev-stages
Build Gentoo stages leveraging crossdev

## Status

- [x] Build and assemble packages to a stage1 [catalyst](https://wiki.gentoo.org/wiki/Catalyst) can leverage
- [x] Update a compatible stage3 image
- [x] Build opensbi + u-boot images and linux kernel + modules
- [x] Assemble bootable images
- [x] Sandboxed cross-compilation via [hakoniwa](https://github.com/aspect-build/hakoniwa)

## Platforms
- SpacemiT K1 (BPI-F3, Milk-V Jupiter, DC Roma II)
- SpacemiT KY-X1 (OrangePi RV2)
- Canaan K230 (CanMV-K230-V1.1, 01Studio CanMV K230)

## Usage

### Sandbox workflow (no root required)
``` sh
# Set up a sandbox and cross-compilation environment
./sandbox-stage.sh setup riscv64
./sandbox-stage.sh prepare
./sandbox-stage.sh setup-crossdev

# Or with a specific board config
./sandbox-stage.sh --board k1 setup

# Manage targets
./sandbox-stage.sh --board k1 target setup
./sandbox-stage.sh target update
./sandbox-stage.sh target pack

# List and manage
./sandbox-stage.sh list
./sandbox-stage.sh target list
```

### Direct workflow (requires root)
``` sh
# Set up crossdev and build a stage
./cross-stage.sh prepare k1
./cross-stage.sh make k1 gentoo-k1
./cross-stage.sh update k1 gentoo-k1
```

### Build bootable images
``` sh
./make-image.sh k1 build gentoo-k1
./make-image.sh k230 build gentoo-k230
./make-image.sh --dry-run k1 build gentoo-k1  # preview without building
```

## Board configuration

Each board is defined in `boards/<name>/`:

```
boards/k1/
  board.toml        # Board config: arch, cflags, repos, packages
  board.sh          # Optional: override build steps
  genimage.cfg      # Partition layout
```

### board.toml

All declarative config lives here. Shared by cross-stage.sh, sandbox-stage.sh, and make-image.sh.

```toml
[board]
name = "k1"
arch = "riscv64"
cflags = "-O3 -march=rv64gcv_zvl256b -pipe"

[repos.opensbi]
url = "https://github.com/cyyself/opensbi"
tag = "k1-opensbi"

[repos.linux]
url = "https://gitee.com/bianbu-linux/linux-6.6.git"
tag = "k1-bl-v2.2.7-release"

[build]
steps = ["checkout", "bootloader", "linux", "root", "boot", "image"]
linux_defconfig = "k1_defconfig"
uboot_defconfig = "k1_defconfig"
opensbi_platform = "generic"
opensbi_extra = "PLATFORM_DEFCONFIG=defconfig LLVM=1"

[packages]
boot = ["sys-apps/busybox", "sys-kernel/dracut"]

[image]
name = "gentoo-linux-k1_dev-sdcard.img"
```

### board.sh (optional overrides)

Define `board_<step>()` to override the default for that step. Call `default_<step>` inside to extend rather than replace.

```bash
# Only override what differs from the defaults
board_root() {
    default_root              # run standard root setup first
    cp -a firmware/* $root/   # then add board-specific firmware
}

board_boot() {
    # Fully custom boot setup (no useful default for this)
    ...
}
```

Steps: `checkout`, `bootloader`, `linux`, `root`, `boot`, `image`

### Adding a new board

1. Create `boards/<name>/board.toml` with your config
2. Create `boards/<name>/genimage.cfg` with your partition layout
3. If needed, add `boards/<name>/board.sh` with overrides for `board_boot()` and any other custom steps

## Dependencies
``` sh
# Needed to build all the stages
emerge crossdev merge-usr git
# Needed to build the bootloader and kernel
emerge u-boot-tools dtc dracut busybox
# Needed to assemble the whole image
emerge genimage xz-utils
# Sandbox mode (instead of bubblewrap)
emerge hakoniwa
```

### For the newcomers
**crossdev** requires a minimum amount of [setup](https://wiki.gentoo.org/wiki/Crossdev#eselect_creation):
```
emerge app-eselect/eselect-repository
eselect repository create crossdev
```

## Limitations

- Some packages are cross-compilation unfriendly and rely on runtime checks (e.g. git iconv checks)
- cross-stage.sh currently only supports riscv64 targets
