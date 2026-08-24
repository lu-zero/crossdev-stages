# qemu-riscv64-alpine

Example board for `ROOTFS_PROVIDER="alpine"`: an Alpine v3.24 riscv64
rootfs unpacked by `apk` inside the sandbox, plus a mainline LTS
kernel, packed as a bare ext4 image for `qemu-system-riscv64 -M virt`.
No bootloader is built; qemu loads the kernel directly.

## Host requirements

None beyond the sandbox itself.  This is the difference from
`qemu-riscv64-debian`: apk is a host-arch binary that only reads and
writes files, and the provider runs it with `--no-scripts`, so no target
binary is ever executed and no qemu-user binfmt registration is needed.

riscv64 has been a supported Alpine architecture since 3.20; an
`ALPINE_BRANCH` older than `v3.20` is refused at the start of `deps`
rather than 404ing partway through.

## Signatures

Package signatures are verified.  `defaults/alpine-keys/riscv64/` is
copied into the rootfs before the first `apk add`, so apk has the
architecture's signing keys and `--allow-untrusted` is never passed.
apk-tools itself is not in ::gentoo, so the `crossdev-stages` overlay
carries `app-arch/apk-tools` and the deps step emerges it; the release
tarball is pinned by the ebuild's Manifest.

## Build

```sh
crossdev-stages image build --board qemu-riscv64-alpine
```

The crossdev toolchain is set up only because `BUILD_STEPS` contains
`kernel`; a rootfs-only variant (no kernel step) would skip it entirely.

## Run

The pack step timestamps and compresses the artifact
(`alpine-linux-qemu-riscv64-rootfs.ext4-<timestamp>.xz`); unpack it first.

```sh
cd ~/.cache/crossdev-stages/builds/qemu-riscv64-alpine/<timestamp>/
unxz alpine-linux-qemu-riscv64-rootfs.ext4-*.xz
qemu-system-riscv64 -M virt -m 2G -smp 4 \
  -kernel gen/boot/Image \
  -append "root=/dev/vda rw console=ttyS0" \
  -drive file=alpine-linux-qemu-riscv64-rootfs.ext4-*,format=raw,if=virtio \
  -netdev user,id=n0,hostfwd=tcp::2222-:22 -device virtio-net-device,netdev=n0 \
  -nographic
```

Root login has an empty password on the console and over ssh
(`ssh -p 2222 root@localhost`), the same permissive policy the Gentoo and
Debian images ship.

## Files

- `board.conf` - provider, branch, kernel, console, runlevels
- `alpine-packages.txt` - packages added next to `alpine-base` (changing
  the list requires recreating the target:
  `crossdev-stages target destroy riscv64-alpine`)
- `genimage.cfg` - single bare ext4, no partition table

## What `--no-scripts` skips

apk stores a package's scriptlets in the root even when told not to run
them, so the `deps` step lists by name every package whose install
scriptlet did not run.  Three are handled in place, because their effects
are structural:

- `busybox` - the applet symlinks (`/bin/ls`, `/sbin/init`, 300-odd
  others) are recreated from `/etc/busybox-paths.d/`, and bbsuid claims
  its eight suid applets
- `alpine-baselayout` - `/etc/shadow` gets its `shadow` group
- `openrc` - its post-install only migrates pre-0.13 `rcS.d`/`rcL.d`
  layouts, so on a fresh root it is already a no-op

Anything else the `deps` step names is the board's to handle from
`post-assemble.sh`.  For this board's package list, nothing else has one.
