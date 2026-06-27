# SiFive HiFive Premier P550 (ESWIN EIC7700X)

Quad-core SiFive P550 (rv64gc + zba + zbb, **no V**) — `eic7700-hifive-premier-p550.dts` upstream since Linux 6.18; ethernet/MMC since 6.19.

## Boot chain

```
Boot ROM (on-die)
  -> reads bootchain from QSPI NOR (pre-flashed by vendor)
     ddr_fw.bin + second_boot_fw.bin + OpenSBI v1.4 + u-boot 2024.01-EIC7X
  -> u-boot `bootflow scan -b` over boot_targets=mmc1 usb nvme0 ahci mmc0
     1. /extlinux/extlinux.conf       ← absent (we don't ship one)
     2. /boot.scr.uimg                ← absent
     3. /EFI/BOOT/bootriscv64.efi     ← GRUB 2.14 → /grub/grub.cfg
        -> grub linux /Image + devicetree /<dtb>
```

GRUB is the primary path.  Vendor u-boot's `bootflow scan -b` bootmeth
priority is hardcoded (extlinux → boot.scr → EFI), so we skip extlinux
entirely and let u-boot fall through to GRUB.  Matches the
RHEL10/Ubuntu/Fedora distro flow on this board (interactive boot menu,
in-place kernel cmdline edits via `e`, easy multi-kernel setups, works
with `efibootmgr` for NVRAM boot entries on boards with persistent
EFI vars).

The cross-prefix builds `riscv64-efi` GRUB modules via
`crossdev --ex-pkg sys-boot/grub` (triggered by `GRUB_PLATFORMS=
riscv64-efi` in `board.conf`).  `post-assemble.sh` embeds enough of
them into `/EFI/BOOT/bootriscv64.efi` (PE32+ RISC-V 64-bit EFI
application, ~700 KiB) to read `grub.cfg` off the bootfs and chain
into the kernel; the rest (~240 .mod files) are staged at
`/grub/riscv64-efi/` for runtime load.

**Our image does not touch QSPI.**  The vendor bring-up chain in QSPI is
re-used as-is.  We only supply the OS storage media — a single GPT image
with `bootfs` (`Image`, DTB, `EFI/BOOT/bootriscv64.efi`, `grub/grub.cfg`,
`grub/riscv64-efi/*.mod`) and `rootfs`.

If QSPI is damaged, see `boards/premier-p550-full/` (future) — that
variant rebuilds the QSPI chain from `eswincomputing/u-boot`,
`eswincomputing/opensbi`, and the two closed firmware blobs from
`sifive/hifive-premier-p550-tools`.

## Flash

The single `gentoo-linux-premier-p550_dev-sdcard.img.xz` works on any
medium u-boot's `boot_targets` covers.

```sh
xzcat gentoo-linux-premier-p550_dev-sdcard.img.xz \
    | sudo dd of=/dev/<target> bs=4M status=progress conv=fsync
```

`/dev/<target>` examples:
- microSD on a host card reader: `/dev/sdX` (whichever your reader maps to)
- NVMe (after removing the M.2 and plugging into a USB-NVMe enclosure): `/dev/sdX`
- eMMC: not directly writable from host — `dd` from the booted board to
  `/dev/mmcblk1` (or whichever sysfs reports), then reboot and remove SD.

The image self-grows on first boot (`grow-rootfs` OpenRC oneshot fills
`rootfs` to the disk end + `resize2fs`).

## Default credentials

- root, empty password (development image — change on first login)
- sshd enabled, `PermitRootLogin yes`, `PermitEmptyPasswords yes`

## Console

`ttyS0 @ 115200 8N1`.  Same UART vendor u-boot uses, no switch needed.

## ISA / GCC

`-march=rv64gc_zba_zbb -mabi=lp64d`.  **Do not** use `-mcpu=sifive-p450`
— P450 is a superset (Zbs, Zicbom, Zihintpause, Zfhmin) and its codegen
will SIGILL on real P550 hardware.  GCC mainline has no `sifive-p550`
entry yet; LLVM does (`SiFiveP500Model`).  Track
`gcc/config/riscv/riscv-cores.def` for the eventual GCC counterpart.

## Known mainline gaps (v7.1.1)

Same gaps RHEL 10 documents (it backports from mainline Linux 7.1):

- **GPU** (Imagination AXM-8-256) — no open Mesa; framebuffer console only
- **NPU** (19.95 TOPS) — vendor blob only
- **HDMI display** — vendor only; use serial console
- **Audio** — disabled in mainline
- **RTC** — time does not survive reboot (RHEL bug, likely same here)

## Vendor firmware version

Vendor QSPI bootchain matters.  Reported by `u-boot version` at boot:

- `2024.09.00-HFP550` — works
- `2024.11.00-HFP550` — NIC regression (downgrade if you hit it)
- `2025.04.00-HFP550` — Fedora minimum
- `2025.11.00-HFP550` — RHEL 10.2 minimum, current recommendation

Update via SiFive's `EsBurn` (USB) or u-boot `es_burn write 0x90000000 flash`.

## First-batch MAC-ID bug

If your board's MAC reads `8c:1f:64:00:00:00`, the factory failed to
program it.  Fix with `EsMacIdUpdateTool` from
`github.com/sifiveinc/hifive-premier-p550-tools`.

## References

- DTS in mainline: `arch/riscv/boot/dts/eswin/eic7700{,-hifive-premier-p550}.dts*`
- Vendor U-Boot fork (for the QSPI variant): https://github.com/eswincomputing/u-boot tag `u-boot-2024.01-EIC7X`
- Vendor OpenSBI fork: https://github.com/eswincomputing/opensbi tag `opensbi-1.3-EIC7X`
- QSPI firmware blobs: https://github.com/sifive/hifive-premier-p550-tools
- EIC7700X TRM: https://github.com/eswincomputing/EIC7700X-SoC-Technical-Reference-Manual
- SiFive product page: https://www.sifive.com/boards/hifive-premier-p550
