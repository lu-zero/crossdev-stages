# SpacemiT K3 (X100 + A100, RVA23 RISC-V)

16-core heterogeneous RISC-V SoC: 8× X100 (performance, RVA23) + 8× A100 (efficiency).
Supports beyond-RVA23 ISA extensions (zvl1024b vector, more).

## Boot chain

```
BROM -> FSBL (U-Boot SPL, DRAM bring-up) -> OpenSBI (fw_dynamic.itb) ->
  U-Boot (u-boot.itb) -> ext4load kernel/dtb/initrd from bootfs -> Linux
```

Vendor variants also support `EDK2 -> GRUB -> Linux` (UEFI) by replacing
the `uboot` partition contents with `edk2.itb`; this board targets the
direct-u-boot path (matches Bianbu Minimal / Debian, no EDK2).

## Dependencies

Sandbox needs `dev-embedded/u-boot-tools` (mkimage), `sys-apps/dtc`,
`sys-fs/genimage`, `sys-fs/dosfstools`.  ESOS auxiliary firmware ships as
a pre-built blob (`firmware/esos.itb`); building it from source would
additionally need `riscv64-elf-gcc[multilib]`.

`override-kernel.sh` disables `CONFIG_GCC_PLUGINS` for gcc-16 compatibility
(plugin API broke after gcc-15).

## Storage layout (Bianbu reference)

K3 storage is **4 K LBA UFS** (vendor Pico-ITX uses KINGSTON TY7B-128).
Partition table (matches Bianbu Minimal — `archive.spacemit.com/image/k3/version/bianbu/v4.0/`):

| name      | offset | size | in GPT  | image                                         |
|-----------|--------|------|---------|-----------------------------------------------|
| env       | 640K   | 64K  | hidden  | `u-boot/env.bin`                              |
| bootinfo  | 1M     | 128K | hidden  | `factory/bootinfo_block.bin`                  |
| fsbl      | 1536K  | 512K | hidden  | `factory/FSBL.bin`                            |
| esos      | 4M     | 3M   | hidden  | `esos.itb`                                    |
| opensbi   | 7M     | 1M   | hidden  | `fw_dynamic.itb`                              |
| uboot     | 8M     | 4M   | hidden  | `u-boot/u-boot.itb`                           |
| ESP       | 12M    | 256M | visible | `esp.vfat` (4K sector FAT16)                  |
| bootfs    | 268M   | 256M | visible | `bootfs.ext4` (**4K block** — see note)       |
| rootfs    | 524M   | rest | visible | `rootfs.ext4`                                 |

`bootfs.ext4` is forced to **`-b 4096`** in `genimage.cfg`: mke2fs's
default `small` template picks 1 K blocks for <512M filesystems, but
u-boot's ext4 driver computes `log2_fs_blocksize = log2(fs_blksz) - log2(dev_sector_size) = 10 - 12 = -2`
and left-shifts blknr by that. On RISC-V the shift amount is masked to
6 bits → `blknr << 62` → sector `2^63`, which fails
`fs_devread outside partition 9223372036854775808`.  Forcing 4 K blocks
matches the underlying UFS LBA and dodges the UB.

## Flashing

Prereqs:

- K3 board in fastboot mode (BOOTSEL + reset).
- `fastboot` CLI on host (`pacman -S android-tools` /
  `apt install fastboot` / `emerge dev-util/android-tools`).
- udev rule for sudoless device access, e.g.
  `/etc/udev/rules.d/51-spacemit-fastboot.rules`:
  ```
  SUBSYSTEM=="usb", ATTR{idVendor}=="0525", MODE="0666", TAG+="uaccess"
  ```

Build + flash:

```
crossdev-stages image build --board k3
crossdev-stages image export --board k3 --all --tar
# scp the tarball to the flashing host if separate
tar xf k3-flash-bundle.tar.xz
cd k3-flash-bundle
./flash.sh
```

`image export --board k3 --all` assembles `k3-flash-bundle/` from the
whitelist in `boards/k3/bundle.list` (build-dir relative paths; a
missing entry fails the export unless `optional:`-prefixed; `@image`
resolves to the packed, timestamped image name).  For K3 that is the
boot blobs flash.sh needs (`u-boot/FSBL.bin`, `u-boot/u-boot.itb`,
`u-boot/env.bin`, `u-boot/bootinfo_spinor.bin`, `factory/FSBL.bin`,
`factory/bootinfo_block.bin`, `fw_dynamic.itb`, `esos.itb`), the
filesystem images (`esp.vfat`, `bootfs.ext4`, `rootfs.ext4`), and
optionally the packed single-disk image.  The flash helpers from
`boards/k3/` (`flash.sh`, `partition_4M.json`, `partition_universal.json`,
`fastboot.yaml`) are copied in on top so the bundle is self-contained.
`--tar` wraps it all into `k3-flash-bundle.tar.xz` — filesystem images
inside stay uncompressed (in-bundle xz breaks fastboot sparse flash).

`flash.sh` flashes **NOR + UFS both** — K3 Pico-ITX has 8 M NOR alongside
UFS, and BROM probes NOR first.  Leaving stale bootloader on NOR (e.g.
from a previous Ubuntu/EDK2 flash) causes BROM to keep loading the NOR
copy and silently bypass UFS.

## Boot environment

`bootfs/env_k3.txt` is the runtime env override read by u-boot from
filesystem.  Intentionally minimal — sets `knl_name` / `ramdisk_name` /
`dtb_dir` / `dtb_name` / `ramdisk_addr` / `loglevel`, plus `commonargs`
(a `setenv bootargs …` line that carries the console and the other
kernel cmdline args).  Does **NOT**
override `set_root_arg` — vendor `env.bin`'s compiled-in default
resolves rootfs via GPT partition uuid at runtime
(`root=PARTUUID=${rootfs_guid}`), which works without initramfs/udev.

DTB lives at `bootfs/spacemit/<kver>/<board>.dtb` (vendor convention).
The `BOOT_DTB_NAME` in `board.conf` is `k3-pico-itx.dtb` — other K3
boards (k3_com260, k3_evb2, …) need their own value.

## Troubleshooting

- **UEFI Shell instead of Linux** — old EDK2 on NOR not overwritten.
  Run `./flash.sh` (it covers NOR), not just `fastboot flash uboot`.
- **`fs_devread outside partition 9223372036854775808`** — bootfs.ext4
  was created without `-b 4096`. Verify `genimage.cfg`.
- **`Cannot open root device "..."`** — `set_root_arg` got overridden
  in env_k3.txt; remove that line so vendor PARTUUID default runs.

## References

- https://www.spacemit.com/community/document/info?nodepath=hardware/key_stone/k3/k3_docs/k3_ds.md
- https://github.com/spacemit-com/K3-Ubuntu-Images
- https://github.com/jing-liu-spacemit/debian-builder (k3-main branch)
- https://archive.spacemit.com/image/k3/version/bianbu/v4.0/
