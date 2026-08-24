# qemu-riscv64-debian

Example board for `ROOTFS_PROVIDER="debian"`: a Debian trixie riscv64
rootfs built by debootstrap inside the sandbox, plus a mainline LTS
kernel, packed as a bare ext4 image for `qemu-system-riscv64 -M virt`.
No bootloader is built — qemu loads the kernel directly.

## Host requirements

The debootstrap second stage and `--include` package configuration run
target binaries in a chroot, so the host needs qemu-user binfmt
registered with the F (fix-binary) flag:

- Arch: `qemu-user-static` + `qemu-user-static-binfmt`
- Debian/Ubuntu: `qemu-user-static` (binfmt registration included)

Without it the second stage fails with "Exec format error".

A host that cannot register binfmt can set
`ROOTFS_SECOND_STAGE="first-boot"` instead, which defers the second stage
to the board; `boards/qemu-riscv64-ubuntu` uses it and its README spells
out what it costs.

## Build

```sh
crossdev-stages image build --board qemu-riscv64-debian
```

The crossdev toolchain is set up only because `BUILD_STEPS` contains
`kernel`; a rootfs-only variant (no kernel step) would skip it
entirely.

## Run

The pack step timestamps and compresses the artifact
(`debian-linux-qemu-riscv64-rootfs.ext4-<timestamp>.xz`); unpack it
first.

```sh
cd ~/.cache/crossdev-stages/builds/qemu-riscv64-debian/<timestamp>/
unxz debian-linux-qemu-riscv64-rootfs.ext4-*.xz
qemu-system-riscv64 -M virt -m 2G -smp 4 \
  -kernel gen/boot/Image \
  -append "root=/dev/vda rw console=ttyS0" \
  -drive file=debian-linux-qemu-riscv64-rootfs.ext4-*,format=raw,if=virtio \
  -netdev user,id=n0,hostfwd=tcp::2222-:22 -device virtio-net-device,netdev=n0 \
  -nographic
```

Root login has an empty password on the console and over ssh
(`ssh -p 2222 root@localhost`) — the image carries the same permissive
sshd drop-in the Gentoo images ship.

## Files

- `board.conf` — provider, suite, kernel, console
- `debian-packages.txt` — extra packages for `debootstrap --include`
  (installed during the second stage; changing the list requires
  recreating the target: `crossdev-stages target destroy riscv64`)
- `genimage.cfg` — single bare ext4, no partition table
