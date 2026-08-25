# qemu-riscv64-fedora

Example board for `ROOTFS_PROVIDER="fedora"`: a Fedora 44 riscv64 rootfs
unpacked from the published container base image, `systemd` and friends
installed on top with dnf5, plus a mainline LTS kernel, packed as a bare
ext4 image for `qemu-system-riscv64 -M virt`.  No bootloader is built;
qemu loads the kernel directly.

## Host requirements

qemu-user binfmt registered with the `F` flag, for the same reason
`qemu-riscv64-debian` needs it: `dnf5` installs the packages in
`fedora-packages.txt` into the root, and rpm runs each package's
scriptlets there under the target architecture.  On Debian:

```sh
apt install qemu-user-binfmt
```

Check it with `cat /proc/sys/fs/binfmt_misc/qemu-riscv64`; `flags:` must
contain `F`.

Emptying `fedora-packages.txt` removes that requirement entirely, at the
cost of an image with no init.  Unpacking the base image executes
nothing.

## What Fedora publishes for riscv64

riscv64 is not a Fedora architecture.  It is not in
`releases/44/Everything/` and not in `fedora-secondary/` either; the
RISC-V SIG composes it separately, publishes images under
`/pub/alt/risc-v/release/44/` and packages from its own koji.  The
provider knows about the different path.

Two consequences worth knowing before you use this for anything real:

- **The packages are unsigned.**  Fedora's own `fedora-riscv.repo` ships
  `gpgcheck=0`, there is no `RPM-GPG-KEY-fedora-44-riscv64`, and the
  RPMs carry no OpenPGP signature.  The `deps` step says so out loud
  every time it installs.  aarch64 and x86_64 have proper signatures and
  the provider checks them.
- **The repository is not pinnable.**  It is
  `repos-dist/f44/latest/riscv64`, and `latest` is a symlink koji moves.
  The base image *is* pinned: fixed URL, fixed sha256, committed in
  `crossdev-stages/src/provider.rs`.  So the seed is reproducible and
  what dnf5 puts on top of it is not.

For riscv64 that committed sha256 is the whole trust anchor, since the
SIG publishes a bare `.sha256` sidecar where the primary composes
publish a clearsigned `CHECKSUM`.

## Build

```sh
crossdev-stages image build --board qemu-riscv64-fedora
```

The crossdev toolchain is set up only because `BUILD_STEPS` contains
`kernel`.  The first build in a fresh sandbox also emerges dnf5 and its
42-package closure, three of which (`dev-libs/libsolv`,
`dev-libs/librepo`, `sys-apps/dnf5`) come from the crossdev-stages
overlay repository because ::gentoo does not carry them.

## Run

The pack step timestamps and compresses the artifact; unpack it first.

```sh
cd ~/.cache/crossdev-stages/builds/qemu-riscv64-fedora/<timestamp>/
unxz fedora-linux-qemu-riscv64-rootfs.ext4-*.xz
qemu-system-riscv64 -M virt -m 2G -smp 4 \
  -kernel gen/boot/Image \
  -append "root=/dev/vda rw console=ttyS0" \
  -drive file=fedora-linux-qemu-riscv64-rootfs.ext4-*,format=raw,if=virtio \
  -netdev user,id=n0,hostfwd=tcp::2222-:22 -device virtio-net-device,netdev=n0 \
  -nographic
```

Root login has an empty password on the console and over ssh
(`ssh -p 2222 root@localhost`), the same permissive policy the Gentoo,
Debian and Alpine images ship.

## Files

- `board.conf` - provider, release, kernel, console
- `fedora-packages.txt` - installed by dnf5 next to the 147 packages the
  base image already has (changing the list requires recreating the
  target: `crossdev-stages target destroy riscv64-fedora`)
- `genimage.cfg` - single bare ext4, no partition table

## Why the base image needs `systemd` added

Fedora builds the container base from a KIWI profile that ignores
`kernel` and installs `systemd-standalone-sysusers` instead of
`systemd`, so the tree has bash, coreutils, glibc, rpm and dnf5 and no
PID 1; the OCI config's `Cmd` is `/bin/bash`.  The profile that does
carry systemd, `Container-Base-Generic-Init`, is not published, and the
Fedora artifacts that boot are Cloud qcow2 and Server raw disk images,
neither of which is a tarball.  So the init comes from
`fedora-packages.txt`, and `deps` warns when the assembled root has
none.
