# Zhihe A210 (a210-evb)

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

LTS alternative: `KERNEL_TAG="osl/a210-6.6.x-lts"` (6.6.142 base) — more
conservative if `osl/a210-mainline` regresses.  Vendor `develop` branches
move; pin to the tags above.

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

```sh
# Board off → hold Flash button → connect USB-C → power on
cd <bundle dir from `crossdev-stages image export --board zhihe-a210 --all`>
sudo ./flash.sh
```

Two-stage fastboot bring-up handled automatically — bootzero-rvbl.bin
brings up DDR, spl-with-fit-rvbl.bin loads SPL+opensbi+u-boot into RAM,
then u-boot's own fastboot accepts the GPT + per-partition flashes.

## Partition layout

GPT, single-slot (A side populated, B reserved for future A/B switch):

| name      | size      | populated | purpose                       |
|-----------|-----------|-----------|-------------------------------|
| factory   | 32K       | no        | OTP / MAC / board info        |
| uboot_env | 32K       | no        | u-boot env (redundant pair)   |
| boot_a    | 256M      | **yes**   | bootfs.ext4 (kernel + DTB)    |
| boot_b    | 256M      | no        | reserved (A/B slot B)         |
| system_a  | 2494M     | **yes**   | rootfs.ext4                   |
| system_b  | 2494M     | no        | reserved (A/B slot B)         |
| app_a/b   | 512M each | no        | reserved (vendor app slots)   |
| data      | rest      | no        | reserved (vendor /data)       |

Partition UUIDs match vendor `gpt_emmc.txt` so vendor recovery tooling
still resolves names.  True A/B redundancy is a follow-up.

## Console

`ttyS4 @ 115200 8N1`.  UART4 in DTS, matches vendor u-boot and bootargs.

## Known gaps

- **NPU** (12 TOPS) — vendor blob only, no open driver.
- **GPU** — vendor Vulkan blob; no Mesa support yet.
- **HDMI/DisplayPort output** — vendor only; use serial console.
- **A/B redundancy** — partition table reserves the slots but only the
  A slot is populated.  Switch follow-up.
- **u-boot SPL RVBL build target** — vendor `CONFIG_BUILD_TARGET="zhihe-rvbl.bin"`
  may produce a pre-wrapped SPL; we re-wrap from raw `u-boot-spl.bin` in
  post-assemble for determinism.

## References

- DTS / ISA: `zhihe-a210-kernel/arch/riscv/boot/dts/zhihe/a210-soc-core.dtsi`
- Kernel defconfig: `zhihe-a210-kernel/arch/riscv/configs/a210_evb_defconfig`
- U-Boot defconfig: `zhihe-a210-u-boot/configs/a210_evb_defconfig`
- OpenSBI override: `zhihe-a210-opensbi/platform/generic/zhihe/a210.c`
- GPT layout: `zhihe-a210-buildroot/board/zhihe/common/images/gpt/gpt_emmc.txt`
- Flash script: `zhihe-a210-u-boot/board/zhihe/common/script/fastboot_images.sh`
- RVBL header: `zhihe-a210-u-boot/board/zhihe/common/script/generate_firmware.sh`
- FIT template: `zhihe-a210-u-boot/board/zhihe/a210-evb/riscv-boot.its`
- Vendor SDK (closed blobs): https://developer.zhcomputing.com/downloads/release/zhihesdk/v2.8.1/
