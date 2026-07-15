# SiFive HiFive Premier P550 (ESWIN EIC7700X)

Quad-core SiFive P550 (rv64gc + zba + zbb, **no V**) — `eic7700-hifive-premier-p550.dts` upstream since Linux 6.18; ethernet/MMC since 6.19.

## Boot chain

```
Boot ROM (on-die)
  -> reads bootchain from QSPI NOR (pre-flashed by vendor)
     ddr_fw.bin + second_boot_fw.bin + OpenSBI v1.6 + u-boot 2024.01-EIC7X
  -> u-boot `bootflow scan -b` over boot_targets=mmc1 usb nvme0 ahci mmc0
     1. /extlinux/extlinux.conf   ← we ship this; first match wins
     2. /boot.scr.uimg
     3. /EFI/BOOT/bootriscv64.efi
```

extlinux is the primary path: vendor u-boot's bootmeth order tries it
first, and its handler env-expands `${ram_size}` (set by the vendor
`dram_init()`) into the kernel cmdline.  `mem=${ram_size}G` comes from
the vendor SDK's own extlinux.conf; mainline-DTB boots without it fault
in `paging_init` (`__memset`, cause=7 store access fault on
PMP-protected memory), so we keep it.  The mainline
`eic7700-hifive-premier-p550.dts` skeleton has no `/memory` or
`/reserved-memory` nodes and relies on u-boot's fixups alone.

**Our image does not touch QSPI.**  The vendor bring-up chain in QSPI is
re-used as-is.  We only supply the OS storage media — a single GPT image
with `bootfs` (`Image`, DTB, `extlinux/extlinux.conf`) and `rootfs`.

If QSPI is damaged, see `boards/premier-p550-full/` (future) — that
variant rebuilds the QSPI chain from `eswincomputing/u-boot`,
`eswincomputing/opensbi`, and the two closed firmware blobs from
`sifiveinc/hifive-premier-p550-tools`.

## Flash

The single `gentoo-linux-premier-p550_dev-sdcard-<timestamp>.img.xz`
works on any medium u-boot's `boot_targets` covers.

```sh
xzcat gentoo-linux-premier-p550_dev-sdcard-<timestamp>.img.xz \
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

## Known mainline gaps (v7.2-rc1)

`CONFIG_ARCH_ESWIN=y` landed in defconfig for v7.2-rc1 (commit by
A. Srinivasan, June 2026).  Remaining gaps:

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
- QSPI firmware blobs: https://github.com/sifiveinc/hifive-premier-p550-tools
- EIC7700X TRM: https://github.com/eswincomputing/EIC7700X-SoC-Technical-Reference-Manual
- SiFive product page: https://www.sifive.com/boards/hifive-premier-p550
