# qemu-riscv64-ubuntu

Example board for `ROOTFS_PROVIDER="ubuntu"`: an Ubuntu noble riscv64
rootfs built by debootstrap inside the sandbox, plus a mainline LTS
kernel, packed as a bare ext4 image for `qemu-system-riscv64 -M virt`.
No bootloader is built; qemu loads the kernel directly.

It also demonstrates `ROOTFS_SECOND_STAGE="first-boot"`, so this board
builds on a host with no qemu-user binfmt registration at all.

## Ubuntu is not Debian

- Everything but amd64 and i386 lives on `http://ports.ubuntu.com/ubuntu-ports`,
  not on archive.ubuntu.com.  The default mirror follows the arch.
- The archive keyring is `app-crypt/ubuntu-keyring`, which installs
  `/usr/share/keyrings/ubuntu-archive-keyring.gpg`.  It is passed as
  `--keyring=` explicitly: debootstrap's own Ubuntu script picks a
  keyring only after an online end-of-life lookup and falls back to the
  removed-keys keyring when that lookup fails.
- Ubuntu has no `stable`/`testing` alias, so `UBUNTU_SUITE` must name a
  codename.

## Second stage: chroot (default) or first boot

`debootstrap --foreign` downloads every .deb and unpacks only the
Priority:required set.  Something still has to run
`/debootstrap/debootstrap --second-stage`.

`ROOTFS_SECOND_STAGE="chroot"` (the default, and what Armbian, Raspberry
Pi OS, debos and vmdb2 all do) runs it in a chroot at build time, so the
image ships finished.  A foreign arch needs qemu-user binfmt registered
with the F flag on the host:

- Arch: `qemu-user-static` + `qemu-user-static-binfmt`
- Debian/Ubuntu: `qemu-user-static`

`ROOTFS_SECOND_STAGE="first-boot"`, which this board uses, skips the
chroot and installs `/sbin/init` as a shim that finishes the bootstrap on
the board.  It is the honest option for a build host that cannot register
binfmt, and it costs:

- The image ships every downloaded .deb, roughly doubling its size until
  the shim's `apt-get clean` runs after a successful second stage.
- The first boot spends several minutes unpacking and configuring
  hundreds of packages over a serial console.
- If it fails you get a half-configured system: the shim writes
  `/etc/.debootstrap-second-stage`, keeps the output in
  `/var/log/crossdev-first-boot.log`, says `dpkg --configure -a` on the
  console, and drops to a root shell rather than continuing.

## Build

```sh
crossdev-stages image build --board qemu-riscv64-ubuntu
```

The crossdev toolchain is set up only because `BUILD_STEPS` contains
`kernel`; a rootfs-only variant would skip it entirely.

## Run

The pack step timestamps and compresses the artifact; unpack it first.

```sh
cd ~/.cache/crossdev-stages/builds/qemu-riscv64-ubuntu/<timestamp>/
unxz ubuntu-linux-qemu-riscv64-rootfs.ext4-*.xz
qemu-system-riscv64 -M virt -m 2G -smp 4 \
  -kernel gen/boot/Image \
  -append "root=/dev/vda rw console=ttyS0" \
  -drive file=ubuntu-linux-qemu-riscv64-rootfs.ext4-*,format=raw,if=virtio \
  -netdev user,id=n0,hostfwd=tcp::2222-:22 -device virtio-net-device,netdev=n0 \
  -nographic
```

The first boot runs the second stage; watch it on the console.  After it
hands over to systemd, root logs in with an empty password.

Networking is not configured: a debootstrap Ubuntu gets netplan with no
YAML, so no interface comes up.  Add a `/etc/netplan/*.yaml` from a
`post-assemble.sh` hook if the board needs one.

## Files

- `board.conf` - provider, suite, second-stage mode, kernel, console
- `ubuntu-packages.txt` - extra packages for `debootstrap --include`
  (installed during the second stage; changing the list requires
  recreating the target: `crossdev-stages target destroy riscv64-ubuntu`)
- `genimage.cfg` - single bare ext4, no partition table
