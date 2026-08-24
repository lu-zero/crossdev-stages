# qemu-riscv64-buildroot

Example board for `ROOTFS_PROVIDER="buildroot"`: buildroot's stock
`qemu_riscv64_virt_defconfig` builds the rootfs, the kernel and OpenSBI,
and this project unpacks the rootfs into `/target`, installs the kernel
into `/boot` and packs a bare ext4 for `qemu-system-riscv64 -M virt`.

## What is different about this provider

Every other provider hands over a package archive to unpack from.
Buildroot is a source build system that produces a toolchain, a rootfs,
and optionally a kernel and a bootloader from one defconfig, so it
overlaps with what this project does rather than plugging into it.

This board takes the overlap the blunt way: buildroot builds
everything, including its own cross toolchain, and crossdev is never set
up. `BUILD_STEPS=(deps assemble pack)` is what says so. Putting
`checkout kernel` back would build a second toolchain to build a second
kernel, which is a real cost and has to be meant.

The consequence to know about: with no crossdev prefix there is no
`riscv64-unknown-linux-gnu-readelf` and no `-gcc` to ask, so the ABI and
ISA checks that run after `assemble` cannot run. They warn and the build
continues. Pointing buildroot at the store prefix
(`BR2_TOOLCHAIN_EXTERNAL`) is what would give them back; see
`RootfsProvider::Buildroot` in `crossdev-stages/src/provider.rs`.

## The two empty fields

`KERNEL_REPO` and `KERNEL_DEFCONFIG` are required of every board by the
parser. This board's kernel comes out of the defconfig, so there is no
repo to clone and no defconfig to write, and the fields say that by
being empty rather than by naming a tree nothing reads.

## Build

```sh
crossdev-stages image build --board qemu-riscv64-buildroot
```

The first build downloads and compiles a full toolchain and userland;
budget an hour or more. Buildroot's package downloads are cached in
`~/.cache/crossdev-stages/buildroot-dl/` and survive `image prune`, so a
rebuild does not refetch them. The source tree and the output tree live
in the build dir (`buildroot/`, `buildroot-out/`) and do not.

`BR2_TARGET_ROOTFS_TAR=y` is appended to the generated `.config` and
checked afterwards: `qemu_riscv64_virt_defconfig` asks for ext2 only,
and a tar is what unpacks into `/target`.

## Run

The pack step timestamps and compresses the artifact; unpack it first.
The kernel to boot with is the one buildroot built.

```sh
cd ~/.cache/crossdev-stages/builds/qemu-riscv64-buildroot/<timestamp>/
unxz buildroot-linux-qemu-riscv64-rootfs.ext4-*.xz
qemu-system-riscv64 -M virt -m 1G -smp 2 \
  -kernel buildroot-out/images/Image \
  -append "root=/dev/vda rw console=ttyS0" \
  -drive file=buildroot-linux-qemu-riscv64-rootfs.ext4-*,format=raw,if=virtio \
  -nographic
```

Root login has no password: that is
`BR2_TARGET_GENERIC_ROOT_PASSWD` in the defconfig, not something this
project writes. Hostname, getty and init system are the defconfig's too,
so `BOOT_HOSTNAME` and `BOOT_SERIAL_TTY` are not set here and would be
ignored with a warning if they were.

## Files

- `board.conf` - provider, defconfig, buildroot ref, kernel name
- `genimage.cfg` - single bare ext4, no partition table
