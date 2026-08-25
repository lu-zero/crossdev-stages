# qemu-riscv64-openwrt

Example board for `ROOTFS_PROVIDER="openwrt"`: an OpenWrt 25.12.5
riscv64 userspace assembled by the official ImageBuilder inside the
sandbox, plus a mainline kernel, packed as a bare ext4 image for
`qemu-system-riscv64 -M virt`.  No bootloader is built; qemu loads the
kernel directly.

Nothing is cross-compiled for the rootfs: ImageBuilder puts together
prebuilt binary packages.  The crossdev toolchain is set up only
because `BUILD_STEPS` contains `kernel`.

## What is pinned

| | |
| --- | --- |
| Release | `25.12.5` |
| Target | `sifiveu/generic` |
| Profile | `sifive_unmatched` |
| ImageBuilder | `openwrt-imagebuilder-25.12.5-sifiveu-generic.Linux-x86_64.tar.zst` |
| sha256 | `6b00a1d4bf409ed2196d01c64fca525331c5df66c9fe52ce04040d8914e0ca98` |

The release directory is immutable.  The tarball is checked against the
`sha256sums` published beside it *and* against `OPENWRT_SHA256`, which
is the value a mirror cannot argue with.  Everything apk then fetches
from the package feeds is signature-checked against the keys inside
that tarball.

The target only decides which userspace binaries are fetched, and every
OpenWrt riscv64 target (`d1`, `sifiveu`, `siflower`, `starfive`) builds
the same `riscv64_generic` packages: `-march=rv64gc -mabi=lp64d`.  The
profile decides which device-specific packages are added, so on a board
that runs its own kernel it is close to cosmetic.

## Build

```sh
crossdev-stages image build --board qemu-riscv64-openwrt
```

The `deps` step needs network: the ImageBuilder tarball is ~40 MB and
the package feeds are fetched on top of it.

## Run

The pack step timestamps and compresses the artifact
(`openwrt-linux-qemu-riscv64-rootfs.ext4-<timestamp>.xz`); unpack it
first.

```sh
cd ~/.cache/crossdev-stages/builds/qemu-riscv64-openwrt/<timestamp>/
unxz openwrt-linux-qemu-riscv64-rootfs.ext4-*.xz
qemu-system-riscv64 -M virt -m 2G -smp 4 \
  -kernel gen/boot/Image \
  -append "root=/dev/vda rw console=ttyS0" \
  -drive file=openwrt-linux-qemu-riscv64-rootfs.ext4-*,format=raw,if=virtio \
  -netdev user,id=n0 -device virtio-net-device,netdev=n0 \
  -nographic
```

Root has no password, which is OpenWrt's own default.  The console
login comes from `/etc/inittab`; `assemble` replaces the OpenWrt
target's own serial line (`ttySIF0`, from the sifiveu images) with the
one `BOOT_SERIAL_TTY` names.

## What does not work out of the box

- **The firewall.**  `firewall4` needs nftables in the running kernel,
  and a mainline `defconfig` builds `nf_tables` as a module that
  OpenWrt's `kmodloader` has no `/etc/modules.d` entry for.  The box
  boots and the console works; `/etc/init.d/firewall` does not.  Fix it
  with a `KERNEL_CONFIG_FRAGMENTS` entry that makes the netfilter
  symbols `=y`.
- **OpenWrt's own kmods.**  Anything `kmod-*` that the profile pulls in
  installs under `/lib/modules/<openwrt kernel version>/` and is dead
  weight next to the modules this board's `kernel` step installs.
- **The network defaults are a router's.**  `/etc/board.d/02_network`
  puts `lan` on `eth0`, and `/bin/config_generate` gives it the static
  `192.168.1.1/24`.  Under `-netdev user` that is not a DHCP client;
  say so in `/etc/config/network` from a `post-assemble.sh` hook if a
  DHCP lease is wanted.

## Files

- `board.conf` - provider, the OpenWrt pin, kernel, console
- `openwrt-packages.txt` - extra packages for `make image PACKAGES=`
  (changing the list requires recreating the target:
  `crossdev-stages target destroy riscv64-openwrt`)
- `genimage.cfg` - single bare ext4, no partition table
