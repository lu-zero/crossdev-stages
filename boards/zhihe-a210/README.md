# Zhihe A210 (a210-dev)

Shanghai Zhihe A210 SoC — 8-core heterogeneous RISC-V: 4× T-Head C920
(OoO) + 4× T-Head C908 (in-order), both at 1.9 GHz, identical ISA.
NPU 12 TOPS, GPU (Vulkan 1.2), LPDDR4X up to 16 GB, eMMC 5.1,
2× GbE, PCIe Gen3, USB-C 3.1 w/ DisplayPort, HDMI 2.0.

## Boot chain

```
BootROM (BOOT_SEL strap)
  -> u-boot SPL  (CONFIG_TEXT_BASE=0x70000800, RVBL header)
  -> OpenSBI fw_dynamic  @ 0x80000000  (dynamic handoff, NOT FW_PAYLOAD)
  -> u-boot 2024.10      @ 0x90000000
  -> riscv-boot.itb FIT  (Image + DTB + fw_dynamic + u-boot)
  -> Linux Image @ 0x80200000, DTB @ 0x8c000000
```

Primary boot media is **eMMC**.  Recovery is **USB fastboot**: hold the
Flash button on power-on (forces BOOT_SEL=000) and host runs `flash.sh`.

## ISA / GCC

`-march=rv64gcv_zvl128b_zba_zbb_zbc_zbs -mabi=lp64d`.

Per `arch/riscv/boot/dts/zhihe/a210-soc-core.dtsi`:

```
riscv,isa-extensions = "i","m","a","f","d","c","v",
                       "zicntr","zicsr","zifencei","zihpm",
                       "zba","zbb","zbc","zbs","svpbmt","sscofpmf";
```

RVV 1.0 with **VLEN=128** (vendor spec sheet).  **Not RVA23** — missing
Zacas, Zfa, Zvfh, Zicond.  Do not pass `-march=rva23u64` here; codegen
will SIGILL on the unsupported instructions.

GCC has no `-mcpu=zhihe-a210` (or `-mcpu=thead-c920` covering this
exact extension set); pass the `-march` string directly.

Same RVV 1.0 vector miscompile pattern as K230 — `board.conf` sets a
per-package fallback for `dev-libs/libgcrypt`:

```sh
WORKAROUND_PKGS=("dev-libs/libgcrypt")
WORKAROUND_CFLAGS=("-O3 -march=rv64gc -pipe")
```

## Vendor sources

All from OSU Open Source Lab GitLab mirrors (Zhihe-blessed forks of
upstream — pinned to `v2.9.0` tags):

| repo      | URL                                                  | tag                 |
|-----------|------------------------------------------------------|---------------------|
| linux     | `git.osuosl.org/osuosl/zhihe-a210-kernel.git`        | `osl/a210-mainline` |
| u-boot    | `git.osuosl.org/osuosl/zhihe-a210-u-boot.git`        | `v2.9.0`            |
| opensbi   | `git.osuosl.org/osuosl/zhihe-a210-opensbi.git`       | `v2.9.0`            |
| buildroot | `git.osuosl.org/osuosl/zhihe-a210-buildroot.git`     | reference only      |

LTS alternative: `KERNEL_TAG="osl/a210-6.6.x-lts"` (6.6.141+ base) — more
conservative if `osl/a210-mainline` regresses, and the branch that the
T7 first-board validation actually ran on (`6.6.141-osl+`, 2026-06-15).
Vendor `develop` branches move; pin to the tags above.

## Closed firmware blobs

Three vendor blobs are **not** in the OSL mirrors and **not** in this
repo.  See `firmware/README.md` for the download path.

- `a210-aon.bin` (~52K) — E902 AON firmware (PMIC, RTC, reboot,
  regulators).  `override-kernel.sh` bakes it into the kernel via
  `CONFIG_EXTRA_FIRMWARE` when present.
- `bootzero-rvbl.bin` — fastboot stage-1 (DDR init).  Needed only for
  first-time bring-up; once vendor u-boot is in eMMC, subsequent flashes
  skip it.
- `bootzero2.bin` — full chip programming chain; not used by our flash.

## Flash

### Bundle layout

`crossdev-stages image export --board zhihe-a210 --all` produces this
tree (until the recursive-export change lands you also need to copy
`u-boot/spl-with-fit-rvbl.bin` from the build dir and drop
`firmware/bootzero-rvbl.bin` manually):

```
<bundle>/
├── flash.sh
├── gentoo-linux-zhihe-a210_dev-emmc.img       # GPT image (uncompressed)
├── bootfs.ext4                                # boot partition (256 MiB)
├── rootfs.ext4                                # rootfs partition
└── u-boot/
    ├── bootzero-rvbl.bin                      # vendor blob (DDR init)
    └── spl-with-fit-rvbl.bin                  # OUR build: SPL + opensbi + u-boot
```

If `image export` gave you `*.img.xz`, decompress and rename so flash.sh
finds it:

```sh
unxz gentoo-linux-zhihe-a210_dev-emmc-*.img.xz
mv   gentoo-linux-zhihe-a210_dev-emmc-*.img  gentoo-linux-zhihe-a210_dev-emmc.img
```

### Host prerequisites

- `fastboot` CLI (`emerge dev-util/android-tools`, `pacman -S android-tools`,
  or `apt install fastboot`)
- USB-A → USB-C cable to the board
- 12 V DC power supply

### Enter fastboot mode

Vendor BOOT_SEL=000 USB recovery path:

1. Board powered off.
2. **Hold the Flash button**, **press Reset**, plug in USB-A to host.
3. Apply 12 V DC.
4. Verify host sees the board: `fastboot devices` — should list one
   device (any string, vendor reports `product: a2*`).

### Flash

```sh
cd <bundle dir>
sudo ./flash.sh
```

`flash.sh` walks the vendor recovery chain — same shape as
`board/zhihe/common/script/fastboot_images.sh` but trimmed to our
4-partition flatten layout:

1. `fastboot flash ram u-boot/bootzero-rvbl.bin` → DDR up
2. `fastboot reboot`
3. `fastboot flash ram u-boot/spl-with-fit-rvbl.bin` → SPL → opensbi → u-boot
4. `fastboot reboot` (now in u-boot fastboot)
5. `dd if=*-emmc.img of=gpt.img bs=512 count=34` (extracted on first run)
6. `fastboot flash gpt    gpt.img`         (17408 B — GPT header only)
7. `fastboot flash boot   bootfs.ext4`
8. `fastboot flash rootfs rootfs.ext4`
9. `fastboot reboot`

Sending the **full** disk image to `fastboot flash gpt` triggers
`(remote: '10205000')` — vendor u-boot's gpt handler expects only the
17408-byte GPT structure (LBA 0–33: protective MBR + GPT header +
entries).  flash.sh extracts that on the fly.

If the board is already in u-boot fastboot mode from an earlier run
(`getvar product` reports `a2*`), stages 1–4 are skipped.

### First-boot env setup (one-time, over serial)

Connect serial console (`ttyS4 @ 115200 8N1`), interrupt u-boot
autoboot, then:

```text
=> setenv bootargs 'console=ttyS4,115200 root=PARTUUID=0510b001-0000-4710-a210-000000000004 rw rootwait earlycon loglevel=4'
=> setenv bootcmd 'ext4load mmc 0:3 ${kernel_addr} Image; ext4load mmc 0:3 ${dtb_addr} a210-dev.dtb; booti ${kernel_addr} - ${dtb_addr}'
=> saveenv
=> boot
```

MAC setup is separate — see `MAC handling` below.

### Dumping eMMC before flashing (optional backup)

Stock A210 u-boot has only `fastboot flash` enabled, **not**
`fastboot fetch` / `oem dump` / UMS, so you **cannot read eMMC back via
fastboot alone**. Two options:

1. **Add UMS to u-boot** — append to `a210_evb_defconfig`:
   ```
   CONFIG_CMD_UMS=y
   CONFIG_USB_FUNCTION_MASS_STORAGE=y
   ```
   Rebuild `spl-with-fit-rvbl.bin`, `fastboot flash ram` + reboot into
   u-boot, then at the prompt:
   ```text
   => ums 0 mmc 0      # whole user area as /dev/sdX
   => ums 0 mmc 0.1    # boot0 hw partition
   => ums 0 mmc 0.2    # boot1 hw partition
   ```
   On the host: `dd if=/dev/sdX of=a210-emmc.img bs=4M conv=fsync status=progress`.
   Dump twice, compare `sha256sum`.

2. **Boot a rescue initramfs via `fastboot flash ram`** (same path as
   VSRVES01's phram-rootfs), then `dd /dev/mmcblk0 | nc host 9000` or
   write to a USB stick. More work; useful if you also want live GPT
   inspection.

## Partition layout

OSL "flatten" layout (per `a210-linux/docs/PARTITIONS.md`) — four
partitions instead of the vendor A/B-redundant nine:

| # | name      | size     | purpose                                          |
|---|-----------|----------|--------------------------------------------------|
| 1 | factory   | 16 KiB   | vendor OTP / MAC / `fnv` factory NVRAM (reserved) |
| 2 | uboot_env | 16 KiB   | u-boot `saveenv` target (reserved)               |
| 3 | boot      | 256 MiB  | ext4 `A210-BOOT`, PARTUUID `…-0003` (Image + DTB) |
| 4 | rootfs    | rest     | ext4 `A210-ROOT`, PARTUUID `…-0004` (userland)   |

Partitions 1–2 are deliberately reserved at the vendor offset/size so
the board's `fnv` and `saveenv` resolve by name; their PARTUUIDs in this
image will differ from the board-burned ones (`bcafe35e-…`, `9d8828c9-…`)
and that is expected — see comments in `genimage.cfg`.

Full A/B + overlay (vendor's `bootcount`/`bootlimit` rollback) is
documented in `a210-linux/docs/AB_LAYOUT.md` as a future migration,
deferred until flatten is bench-validated end-to-end.

## Provisioning flow

OSL fleet install is **netboot installer → eMMC flatten image**, per
board, manual over serial (per `PROVISION_RUNBOOK.md`):

1. Install server stages `Image`, `a210-dev.dtb`, `installer.cpio.gz`
   over TFTP and a rootfs tarball over NFS.
2. On each board: interrupt U-Boot, `dhcp` + `tftpboot` the three
   payloads, `booti` with `install_server=<IP> board_id=<NN>` on the
   cmdline.  The Debian initramfs installer (busybox, e2fsprogs, gdisk,
   nfs-common, udhcpc) DHCPs `eth0`, NFS-mounts the export, runs
   `partition-emmc.sh`, writes Image + dtb + rootfs, stamps
   `/etc/osl/board-id`, and reboots.
3. Operator persists the production `bootcmd`/`bootargs` (see
   `board.conf` header) and the MAC pair, then `boot`.

That flow is what OSL deploys to a full fleet.  Single-board users
should just `dd` the produced eMMC image instead — vendor `booti`
accepts our unsigned `Image` directly (vendor confirmed 2026-05-28).

## MAC handling

Factory MAC is **not** present in eFuse (vendor-confirmed); the
`factory` partition stays empty out of the box.  Two options per
`MAC_SCHEME.md`:

- **Durable** (recommended): U-Boot factory-NVRAM — `fnv erase`,
  `env set ethaddr 48:da:??:??:??:??`, `env set eth1addr 48:da:…`,
  `fnv save`.  Prefix `48:da` is locally-administered.  **eth1 MUST
  differ from eth0** — vendor's docs example mistakenly reused one MAC.
- **Runtime fallback**: `osl-setmac.service` in the OSL Debian overlay
  derives both MACs by SHA1 over `/etc/osl/board-id` (or
  `machine-id`).  Whether we replicate that service in our image is
  out of scope here.

## Console

`ttyS4 @ 115200 8N1`.  UART4 in DTS, matches vendor u-boot and bootargs.

## Known gaps

- **NPU** (12 TOPS) — vendor blob only, no open driver.
- **GPU** — vendor Vulkan blob; no Mesa support yet.
- **HDMI/DisplayPort output** — vendor only; use serial console.
- **A/B redundancy** — flatten layout intentionally drops vendor A/B;
  see `AB_LAYOUT.md` for the future migration plan.
- **MAC fallback service** — `osl-setmac.service` (OSL Debian overlay)
  not yet ported to our Gentoo image; durable `fnv ethaddr/eth1addr`
  is the recommended path.
- **u-boot SPL RVBL build target** — vendor `CONFIG_BUILD_TARGET="zhihe-rvbl.bin"`
  may produce a pre-wrapped SPL; we re-wrap from raw `u-boot-spl.bin`
  in post-assemble for determinism.

## References

- DTS / ISA: `zhihe-a210-kernel/arch/riscv/boot/dts/zhihe/a210-soc-core.dtsi`
- Kernel defconfig: `zhihe-a210-kernel/arch/riscv/configs/a210_evb_defconfig`
- U-Boot defconfig: `zhihe-a210-u-boot/configs/a210_evb_defconfig`
- OpenSBI override: `zhihe-a210-opensbi/platform/generic/zhihe/a210.c`
- GPT layout: `zhihe-a210-buildroot/board/zhihe/common/images/gpt/gpt_emmc.txt`
- Flash script: `zhihe-a210-u-boot/board/zhihe/common/script/fastboot_images.sh`
- RVBL header: `zhihe-a210-u-boot/board/zhihe/common/script/generate_firmware.sh`
- FIT template: `zhihe-a210-u-boot/board/zhihe/a210-evb/riscv-boot.its`
- Partition layout: `a210-linux/docs/PARTITIONS.md`, `docs/AB_LAYOUT.md`
- Provisioning: `a210-linux/docs/PROVISION_RUNBOOK.md`
- MAC scheme: `a210-linux/docs/MAC_SCHEME.md`
- T7 validation: `a210-linux/docs/T7-VALIDATION.md` (PASSED 2026-06-15)
- Vendor SDK (closed blobs): https://developer.zhcomputing.com/downloads/release/zhihesdk/v2.8.1/
