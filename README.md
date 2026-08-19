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
| k3 | riscv64 | spacemit 6.18 | OpenSBI + U-Boot | `-O3 -march=rva23u64` | stable |
| k230 | riscv64 | canaan (hdmi) | OpenSBI (payload) + U-Boot | `-O3 -march=rv64gcv_zvl128b` | stable |
| ky-x1 | riscv64 | spacemit 6.6 | OpenSBI + U-Boot | `-O3 -march=rv64gcv_zvl256b` | stable |
| blackhole | riscv64 | tenstorrent | OpenSBI (jump, PCIe BAR) | `-O3 -march=rv64gcv_zvl512b` | stable |
| odroid-m1 | aarch64 | mainline v7.0 | TFA + U-Boot + rkbin | `-O3 -mcpu=cortex-a55+crc+crypto` | testing |
| odroid-m1s | aarch64 | mainline v7.0 | TFA + U-Boot + rkbin | `-O3 -mcpu=cortex-a55+crc+crypto` | testing |
| odroid-m2 | aarch64 | mainline v7.0 | TFA + U-Boot + rkbin | `-O3 -mcpu=cortex-a76.cortex-a55+crc+crypto` | testing |
| odroid-xu4 | armv7a | mainline v7.2 | signed BL1/BL2/TZSW + U-Boot | `-O2 -march=armv7ve -mtune=cortex-a15.cortex-a7 -mfpu=neon-vfpv4` | testing |
| pentium-mmx | i586 | mainline v6.18 | BIOS (no firmware) | `-O2 -march=pentium-mmx` | testing |

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
# (--board bakes the board's CFLAGS into the target make.conf, so
#  @system/@world rebuilds use them; omit for the arch baseline)
crossdev-stages target setup --arch <ARCH>
crossdev-stages target stage1 --board <BOARD>
crossdev-stages target update --board <BOARD>

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
| `BOARD_ARCH` | yes | Target architecture (`riscv64`, `aarch64`, `armv7a`, `i586`, `i686`) |
| `CROSS_COMPILE` | yes | Toolchain prefix (e.g. `riscv64-unknown-linux-gnu-`) |
| `KERNEL_REPO` | yes | Kernel source repository URL |
| `KERNEL_DEFCONFIG` | yes | Kernel defconfig name |
| `KERNEL_CONFIG_FRAGMENTS` | no | Config fragments to apply after the defconfig |
| `CHOST` | no | Override derived CHOST triple (default: auto from arch) |
| `BOARD_CFLAGS` | no | Board-specific CFLAGS (default: arch default) |
| `BOARD_GCC_VERSION` | no | Pin gcc: `15` (slot), `15.2` (prefix), or exact version (default: highest installed slot) |
| `KERNEL_TAG` | no | Kernel git ref (default: top-level `TAG`) |
| `KERNEL_ARCH` | no | Linux `ARCH=` value (default: auto from `BOARD_ARCH`) |
| `BUILD_STEPS` | no | Build pipeline steps (default: deps checkout bootloader kernel assemble pack); custom step names require a matching `override-<step>.sh` hook |
| `BOOT_PIPELINE` | no | Ordered bootloader stages (default: `("opensbi" "uboot" "syslinux" "grub")`; `()` = none) |
| `BOOT_EXTLINUX` | no | `true` makes `assemble` write `/extlinux/extlinux.conf` |
| `BOOT_APPEND` | no | Kernel arguments added to that entry |
| `BOOT_DTB_NAME` | no | DTB to boot, when `BOARD_DTB_GLOB` matches more than one |
| `ISA_STRICT` | no | `false` downgrades an unrunnable binary to a warning (default: fail) |
| `ROOTFS_PROVIDER` | no | Who fills the image rootfs: `gentoo` (default; stage3 + cross-emerge + OpenRC), `debian` or `ubuntu` (debootstrap + systemd), `alpine` (apk + OpenRC), `fedora` (container base image + dnf5 + systemd), `buildroot` (defconfig build), or `none` (board hooks own it) |
| `DEBIAN_SUITE` | no | debian provider: suite to debootstrap (default `stable`) |
| `DEBIAN_MIRROR` | no | debian provider: mirror URL (default `https://deb.debian.org/debian`) |
| `UBUNTU_SUITE` | yes for `ubuntu` | Suite codename, e.g. `noble`; Ubuntu has no rolling alias to default to |
| `UBUNTU_MIRROR` | no | ubuntu provider: mirror URL (default `http://ports.ubuntu.com/ubuntu-ports`, or `http://archive.ubuntu.com/ubuntu` on amd64/i386) |
| `ROOTFS_SECOND_STAGE` | no | debootstrap providers: `chroot` (default, build-time, needs qemu-user binfmt) or `first-boot` (deferred to the board) |
| `ALPINE_BRANCH` | no | alpine provider: release branch (default `v3.24`; riscv64 needs `v3.20` or later) |
| `ALPINE_MIRROR` | no | alpine provider: mirror URL (default `https://dl-cdn.alpinelinux.org/alpine`) |
| `ALPINE_REPOS` | no | alpine provider: repositories under the branch (default `main community`) |
| `FEDORA_RELEASE` | no | fedora provider: release to unpack (default `44`; only pinned releases are accepted) |
| `FEDORA_MIRROR` | no | fedora provider: mirror URL (default `https://dl.fedoraproject.org/pub`) |
| `BUILDROOT_DEFCONFIG` | with `buildroot` | A file in `boards/<board>/` if one is there, otherwise a name in buildroot's own `configs/` |
| `BUILDROOT_REPO` | no | buildroot provider: git repo (default `https://gitlab.com/buildroot.org/buildroot.git`) |
| `BUILDROOT_TAG` | no | buildroot provider: release tag, branch or commit SHA (default `master`, warned as unpinned) |
| `OPENSBI_FW_TYPE` | no | OpenSBI firmware type: `dynamic` (default), `jump`, `payload` |
| `OPENSBI_MAKE_FLAGS` | no | Extra opensbi make arguments |
| `U_BOOT_MAKE_FLAGS` | no | Extra u-boot make arguments |
| `GRUB_PLATFORMS` | no | GRUB platform (e.g. `pc`); enables the `grub` stage (grub-mkimage) |
| `GRUB_MODULES` | no | GRUB modules embedded in core.img (default: BIOS boot set) |
| `SYSLINUX_REPO` / `SYSLINUX_TAG` | no | SYSLINUX source repo + tag; enables the `syslinux` stage |
| `TFA_REPO` / `TFA_TAG` / `TFA_PLAT` | no | ARM Trusted Firmware-A (BL31) repo, tag (default `master`), platform |
| `RKBIN_REPO` / `RKBIN_TAG` / `RKBIN_DDR` | no | Rockchip blob repo, tag (default `master`), DDR-init blob glob |
| `FIP_REPO` / `FIP_TAG` | no | Amlogic boot-FIP packaging repo, tag (default `master`) |
| `FIRMWARE_REPO` / `FIRMWARE_TAG` | no | Firmware repo cloned to `/build/firmware` (tag default: `TAG`) |
| `BOARD_FIRMWARE_OVERLAY` | no | Path in that repo whose *contents* go to `/lib/firmware` |
| `FIRMWARE_DIRS` | no | Directories in that repo copied to `/lib/firmware/<dir>`, path preserved |
| `COMPRESSION` | no | Image compression: `xz` (default), `gz`, `none` |
| `TAGS` | no | Labels: arch, SoC, vendor, and `testing`. What CI selects on |
| `DESCRIPTION` | no | One-line note shown by `board info` |

### Rootfs providers

`ROOTFS_PROVIDER` decides who fills the image rootfs. `gentoo` (the default)
seeds a stage3 and cross-emerges into it. `debian` and `ubuntu` run
debootstrap inside the sandbox. `alpine` unpacks the root with `apk`, which
runs on the host arch, so it needs no emulation at all. `fedora` unpacks a
pinned container base image and installs on top of it with dnf5. For all four
the board's extra packages come from
`boards/<name>/<provider>-packages.txt`, one name per line. `buildroot` runs
buildroot's own defconfig build and unpacks the rootfs tar it emits, so the
package set is the defconfig and nothing else. `none` leaves it to board
hooks.

The debootstrap providers differ only in keyring, mirror and suite naming:
Ubuntu keeps everything but amd64/i386 on `ports.ubuntu.com`, uses
`app-crypt/ubuntu-keyring`, and has no rolling alias, so `UBUNTU_SUITE` must
name a codename.

`debootstrap --foreign` downloads every .deb and unpacks only the
Priority:required set, so something still has to run the second stage.
`ROOTFS_SECOND_STAGE="chroot"` (the default) runs it in a chroot at build
time and ships a finished image; a foreign arch needs qemu-user binfmt
registered with the `F` flag on the host. `ROOTFS_SECOND_STAGE="first-boot"`
is the opt-in for a host that cannot do that: `assemble` installs a
`/sbin/init` shim that finishes the bootstrap on the board instead. The image
then carries every downloaded .deb, roughly doubling its size until the shim
runs `apt-get clean` after a successful second stage, and the first boot
spends minutes configuring packages over the serial console. A failure there
writes `/etc/.debootstrap-second-stage`, keeps the output in
`/var/log/crossdev-first-boot.log`, and drops to a root shell.

### Board tags

`TAGS` labels a board by what it is -- architecture, SoC, vendor -- so a
selection can be written once instead of listing board names:

```
TAGS=("aarch64" "rockchip" "rk3588" "odroid" "testing")
```

`board list` prints them, and CI filters on them. A tag is not inherited
through `INCLUDE`: a board states its own list in full, because a tag is an
identity and identity is the one thing a family should not hand down.

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
DEFAULT linux
TIMEOUT 30
LABEL linux
    MENU LABEL <DESCRIPTION, or the board name>
    LINUX /<BOOT_KERNEL_NAME>
    FDT /<dtb>
    APPEND root=<BOOT_ROOT_DEV> rw rootwait rootfstype=ext4 console=<BOOT_CONSOLE> <BOOT_APPEND>
```

The DTB is the basename of `BOARD_DTB_GLOB`, which on most boards already names
exactly one file; a board that copies a whole directory of them sets
`BOOT_DTB_NAME`. A board needing a different shape -- several labels, a kernel
named by version -- writes its own file from `post-assemble.sh` instead.

### A shell inside the image

`chroot` opens a shell in the board's own rootfs -- the filesystem that goes on
the card -- where `/bin/bash` is a riscv64 or aarch64 executable:

```
crossdev-stages chroot --board k230
crossdev-stages chroot --board k230 -- emerge --info
```

Portage runs in there:

```
Portage 3.0.79 (python 3.14.6, gcc-16, glibc-2.43-r2, ... riscv64)
```

Nothing is copied into the rootfs to make this work. Linux registers qemu's
binfmt_misc handlers with the `F` flag, which opens the interpreter once and
keeps the descriptor in the kernel, so it stays reachable from a mount
namespace that cannot see the host's `/usr`. Where a handler is missing or was
registered without `F`, the command says so and gives the explicit
`qemu-<arch>-static -L <rootfs>` form instead of failing obscurely.

It prefers the built image tree and falls back to the shared target stage, so
it works before an image has ever been packed.

Distinct from `enter` below, which opens a shell in the container the *build*
runs in: the build host, with the cross compiler, and the target mounted at
`/target`.

### Debugging a build
### Debugging a build

`enter` opens a shell in the very container a build step runs in -- same
rootfs, same cross toolchain on PATH, same `/target`, `/build`, `/scripts` and
`/cache` mounts:

```
crossdev-stages enter --board k230
crossdev-stages enter --board k230 -- emerge --info
```

The target stage and the build directory are attached when they exist, so a
board that has never been built still opens.

### What is in a binary package cache

Portage decides a binary package fits by matching CHOST, KEYWORDS, USE and
CPU_FLAGS_X86, never CFLAGS, and it records neither the compiler nor the libc
that produced the package. Three layers close that, and each is allowed to be
wrong in one direction only:

| Layer | Answers | May be wrong how | Fails a build? |
|---|---|---|---|
| Key | May these two builds share a cache at all? | Too strict only | No |
| Stamp | What is in this cache, and is it still one set? | Stale only | No |
| Measurement | Does the image we are about to ship work? | Not at all | Yes |

The key is `binpkgs/<chost>/<key>`, where `<key>` hashes the board's CFLAGS
*and its per-package workarounds*: `WORKAROUND_PKGS` builds named packages with
different flags, and two boards agreeing on CFLAGS but differing in workarounds
would otherwise swap binaries neither asked for. The toolchain store keys on
the CFLAGS alone, because a workaround changes what some packages are compiled
with and not what compiles them.

The stamp is portage's own `Packages` header -- the same fields Gentoo's
binhost publishes, PROFILE, ELIBC, CHOST, REPO_REVISIONS -- plus a `.build-env`
beside it naming the toolchain, which portage does not record. `store list`
shows both, and a build prints one line when the toolchain has moved since the
cache was last written to. Nothing here invalidates a cache: a stamp that
triggers rebuilds is a cache key with worse ergonomics.

The measurement is the ABI and ISA checks below, against the finished image.
They are the only thing that can fail a build, because they are the only thing
that reads the artifact.

### ABI verification
### ABI verification

Nothing in a binary package records the libc or the compiler that produced it.
Portage stores CFLAGS, CHOST, USE and the sonames a file needs, but not one
symbol version and not the toolchain, so a package cached from a different
prefix installs without complaint.

That matters because glibc and libstdc++ are backward compatible and not
forward compatible: a binary built against glibc 2.43 asks the loader for
`GLIBC_2.43`, and on an image carrying 2.41 it does not start.

So rather than key the cache on a proxy for the toolchain, `assemble` reads
what every binary in the image actually asks for (`.gnu.version_r`) and checks
the libraries the image ships actually define it (`.gnu.version_d`). Exact,
covers glibc, libstdc++, libgcc and anything else versioned at once, and it
works on every architecture.

### ISA verification

Portage decides a binary package fits by matching CHOST, KEYWORDS, USE and
CPU_FLAGS_X86. It never matches CFLAGS, so a package built for one board
installs into another that cannot run it and nothing says so until the hardware
hits an illegal instruction.

`assemble` therefore reads `Tag_RISCV_arch` out of every ELF file in the image
and compares it against what the board's own toolchain emits -- obtained by
compiling an empty file with the board's CFLAGS, so `-march=rv64gcv_zvl256b`
and `-march=rva23u64` expand exactly the way that compiler expands them, with
no table to keep in step. A binary using an extension the board lacks is
an error: it faults on the hardware, so the build stops. `ISA_STRICT="false"`
downgrades it to a warning while a board is being brought up. A binary merely built
without some of the board's extensions is reported as slower, not broken.

The same scan runs on demand against an existing build or target stage:

```
crossdev-stages verify --board k230
crossdev-stages verify --board k230 --strict
```

RISC-V only: it is the architecture that records its ISA in the ELF and has
extensions a board can genuinely lack.

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

### Toolchain version pins

Every portage root the tool manages (host sandbox, crossdev prefix,
target sysroot) gets `package.mask/pin-{gcc,llvm}` +
`package.unmask/pin-{gcc,llvm}`: gcc is pinned to
`=sys-devel/gcc-${BOARD_GCC_VERSION}*` (host sandbox keeps its stage3
default), llvm-core/* to a single slot hardcoded in
`portage::LLVM_SLOT`.  This keeps `target update` and later emerges
from silently jumping gcc majors (binpkg ABI breakage) or mixing llvm
slots.  Override by editing the pin files in the respective
`etc/portage/`, or change `BOARD_GCC_VERSION` — the files are rewritten
on the next prepare/crossdev/stage run.

### The `crossdev-stages` portage overlay

The ebuilds ::gentoo does not carry live in their own repository,
[crossdev-stages-overlay](https://github.com/OctopusET/crossdev-stages-overlay).
`defaults/overlay.conf` pins its URL and revision; `sandbox prepare`
checks it out at `/var/db/repos/crossdev-stages` inside the sandbox and
writes the repos.conf entry.  Separate because ebuilds carry their
upstream licenses (::guru's are GPL-2) and this repository is
Apache-2.0.

`OVERLAY_REPO` also accepts a path to a local clone, which is
bind-mounted read-only and copied in; leave it empty to build without
the overlay.

`app-arch/apk-tools` is emerged automatically: `ROOTFS_PROVIDER="alpine"`
needs a host-arch apk to unpack a foreign-architecture Alpine root, and
::gentoo has none.  Category follows ::gentoo's own placement of other
distributions' package managers, `app-arch/dpkg` and `app-arch/rpm`.
Built from the upstream release tarball with the checksum in the ebuild's
Manifest, so portage records it in the VDB like any other package.

### Optional: coprocessor firmware (K1/K3 ESOS)

The overlay carries opt-in (p.masked) ebuilds for SpaceMIT
coprocessor firmware:

- `sys-firmware/esos` (USE=k1|k3) - RT-Thread firmware from a single
  upstream tree, chip selected at build time
- `sys-firmware/esos-lite` - K3 PM mini-blob (build-time dep of esos[k3])

To install on a target sysroot (`package.unmask`/`package.accept_keywords`
are directories; live ebuilds also need an explicit `**` keyword):

```sh
echo 'sys-firmware/esos' > /etc/portage/package.unmask/esos
echo 'sys-firmware/esos-lite' > /etc/portage/package.unmask/esos-lite   # k3 only
echo 'sys-firmware/esos **' > /etc/portage/package.accept_keywords/esos
echo 'sys-firmware/esos-lite **' > /etc/portage/package.accept_keywords/esos-lite
crossdev -t riscv64-elf -s4   # baremetal toolchain, ESOS-specific
USE=k1 ROOT=$SYSROOT emerge -av sys-firmware/esos    # K1 board
USE=k3 ROOT=$SYSROOT emerge -av sys-firmware/esos    # K3 board
```

Patches and final build wiring still in progress — current ebuilds are
scaffolding.

## Limitations

- Some packages are cross-compilation unfriendly and rely on runtime checks (e.g. git iconv checks)
