# ODROID-XU4 (Exynos 5422)

Hardkernel ODROID-XU4: Samsung Exynos 5422, 4x Cortex-A15 + 4x Cortex-A7
big.LITTLE, Mali-T628 MP6, 2 GB LPDDR3, gigabit ethernet behind USB 3.0.  The
first 32-bit ARM board here.

Kernel and U-Boot are both mainline.  The only vendor pieces are three
Samsung-signed binaries that have no public source at all.

## Boot chain

| Stage | Source | Blob? |
|---|---|---|
| iROM | on-die | fixed |
| BL1 | `hardkernel/u-boot`, `sd_fuse/bl1.bin.hardkernel` | yes, Samsung-signed |
| BL2 | `hardkernel/u-boot`, `sd_fuse/bl2.bin.hardkernel.720k_uboot` | yes, Samsung-signed |
| U-Boot | mainline v2026.07, `odroid-xu3_defconfig` | no |
| TrustZone SW | `hardkernel/u-boot`, `sd_fuse/tzsw.bin.hardkernel` | yes, Samsung-signed |
| Linux | mainline, `arch/arm/boot/dts/samsung/exynos5422-odroidxu4.dts` | no |

The iROM verifies BL1's signature, BL1 verifies BL2, and BL2 brings up DRAM and
installs the TrustZone monitor that provides PSCI.  U-Boot has no SPL for
Exynos 5422 and could not replace BL2 even if it did: the signing key is
Samsung's.  So these three stay binary, and `post-checkout.sh` clones them from
`hardkernel/u-boot` branch `odroidxu4-v2017.05` (`EXYNOS_BLOB_REPO` /
`EXYNOS_BLOB_TAG` in `board.conf`).  The 2017 U-Boot sources next to them are
ignored.

There is no `odroid-xu4_defconfig` in mainline U-Boot.  `odroid-xu3_defconfig`
is the one, and it says so: `CONFIG_IDENT_STRING=" for ODROID-XU3/XU4/HC1"`.

## SD card layout

Straight from Hardkernel's `sd_fuse/sd_fusing.sh`, 512-byte sectors:

| Sector | Byte | Size | Contents |
|---|---|---|---|
| 0 | 0 | 512 | MBR |
| 1 | 512 | 15360 | BL1 |
| 31 | 15872 | 14592 | BL2 |
| 63 | 32256 | 720 KiB window | U-Boot |
| 1503 | 769536 | 262144 | TrustZone monitor |
| 6272 | 3211264 | 16 KiB | U-Boot environment |
| 16384 | 8 MiB | 256 MiB | boot partition, ext4 |
| | 264 MiB | rest | root partition, ext4 |

Three things to know about that table:

- **MBR, never GPT.** BL1 sits on sector 1, which is exactly where a GPT keeps
  its header, and sectors 2..33 hold BL2 where the GPT partition array would
  go.  A GPT-partitioned card cannot boot.
- **The BL2 blob fixes the U-Boot window.** `bl2.bin.hardkernel.720k_uboot`
  loads exactly 720 KiB from sector 63 and then jumps to the monitor at sector
  1503.  Hardkernel's older `odroidxu3-v2012.07` BL2 uses 328 KiB and sector
  719 instead; Arch Linux ARM and postmarketOS ship a third variant that uses
  1 MiB and sector 2111, which is the layout mainline U-Boot's own
  `include/configs/odroid_xu3.h` assumes in its DFU tables.  All three are 14592
  bytes and none of them is interchangeable.  The pairing chosen here
  (`720k_uboot` + tzsw at 1503) is the one Armbian fuses.
  `post-bootloader.sh` fails the build if U-Boot does not fit the 720 KiB
  window rather than shipping a card that goes quiet after BL2.
- **The environment offset moved.** Mainline U-Boot puts it at
  `CONFIG_ENV_OFFSET=0x310000` (sector 6272).  Hardkernel's 2017 fork used
  sector 2015.  The first partition therefore has to start past 3.1 MiB; 8 MiB
  here.

`post-bootloader.sh` assembles sectors 1..2014 into one flat
`exynos/sd-boot.bin` so `genimage.cfg` writes a single raw region and never has
to express the sub-sector layout.  BL1 as shipped on the 2017 branch is 15616
bytes, but only the first 15360 are BL1; Hardkernel's own script lets BL2
overwrite the trailing 256 bytes, and the 2012 branch ships that same BL1 as
exactly 15360 bytes byte for byte, so the assembly takes 30 sectors.
postmarketOS truncates it to 15360 the same way.

## Booting

Mainline U-Boot's odroid-xu3 target still selects `CONFIG_DISTRO_DEFAULTS`,
whose `BOOT_TARGET_DEVICES` walks mmc2 (the SD slot), mmc1 and mmc0 looking for
`extlinux/extlinux.conf`.  Hardkernel's `boot.ini` is a fork-only feature and is
not needed.  `DISTRO_DEFAULTS` also selects `CMD_BOOTZ` on 32-bit ARM
(`select CMD_BOOTZ if ARM && !ARM64 && LMB` in `boot/Kconfig`), so `pxe_utils`
falls back to `bootz` and a raw `zImage` boots without being wrapped as a
uImage.

Serial console is the 4-pin debug header, `serial@12C20000` = `ttySAC2`, at
115200 8N1.  eMMC is `mmc_0` and holds the `mmc0` DT alias, so the SD slot
(`mmc_2`, unaliased) lands on `mmcblk1` whether or not an eMMC module is
fitted.

`U_BOOT_TAG` must stay at **v2024.10 or newer**.  `board/samsung/smdk5420/Kconfig`
had `SYS_BOARD` defaulting to `"smdk5420"` instead of `"odroid"` from
`f76750d11133` (first released in v2022.04) until Anand Moon's `dd6fff3d83ed`
(first released in v2024.10); in between, U-Boot loads the wrong DTB and the
board does not boot.  The XU3/XU4 board code lives in `board/samsung/smdk5420/`,
not `board/samsung/odroid/`, which is exynos4412.

## Hardware support

All of this is in `exynos_defconfig`; `multi_v7_defconfig` also boots but is a
far larger kernel for no gain on a single-board target.

| Block | Driver | Config |
|---|---|---|
| Display / HDMI | `exynos-drm` (FIMD, mixer, HDMI) | `DRM_EXYNOS_{FIMD,MIXER,HDMI}=y` |
| GPU (Mali-T628 MP6) | `panfrost` | `DRM_PANFROST=y` |
| USB 3.0 | `dwc3` on both `usbdrd_dwc3_{0,1}`, `dr_mode = "host"` | `USB_DWC3=y` |
| USB 2.0 | `ehci-exynos`, `ohci-exynos` | `USB_{EHCI,OHCI}_EXYNOS=y` |
| Gigabit ethernet | Realtek RTL8153 `0bda:8153` on the dwc3 root hub, `r8152` | `USB_RTL8152=y` |
| eMMC / SD | `dw_mmc-exynos` | `MMC_DW_EXYNOS=y` |
| Thermal | 5x TMU (`exynos5420-tmu-ext-triminfo`) | `CPU_THERMAL=y` |
| CPU DVFS | `cpufreq-dt` over both clusters' OPP tables | `CPUFREQ_DT=y`, `ENERGY_MODEL=y` |
| Memory bus DVFS | `exynos5422-dmc`, NoC/PPMU counters | `EXYNOS5422_DMC=y` |
| Audio | `snd-soc-odroid` over i2s0 into the HDMI codec | `SND_SOC_ODROID=y` |
| PWM fan | `pwm-fan` on PWM 0 | `SENSORS_PWM_FAN=y` |
| Blue LED | `pwm-leds` on PWM 2, heartbeat trigger | `LEDS_PWM=y` |
| Watchdog | `s3c2410_wdt` | `S3C2410_WATCHDOG=y` |
| RTC | `rtc-s3c` clocked off the S2MPS11 | `RTC_DRV_S3C=y` |

Both clusters are described: `exynos5422-cpus.dtsi` gives the eight CPUs and
`exynos5800.dtsi` extends the A15 table to 2.0 GHz and the A7 table to 1.4 GHz.
`SCHED_MC=y` and `ENERGY_MODEL=y` are set, so this is scheduler-driven
big.LITTLE with EAS, not the old cluster switcher.  `CONFIG_BIG_LITTLE` selects
MCPM, which is what brings the second cluster up; the `bL_switcher` is not used.

Mainline genuinely boots this SoC today.  Krzysztof Kozlowski (the Exynos
maintainer) runs a buildbot at `krzk.eu` that boot-tests real Exynos 5422
hardware; builder `boot-odroid-hc1-exynos` passed all 49 steps on
`7.1.0-rc7-next-20260611` with 8 CPUs online, 5 thermal zones, DRM, panfrost,
USB and gigabit ethernet over an NFS root.  Note it is an HC1, not an XU4 - the
`boot-odroid-xu3-*` builders have been dead since 2019 - and its last run was
June 2026.

## Gotchas

- **U-Boot cannot netboot as configured.**  `odroid-xu3_defconfig` has
  `CONFIG_USB_HOST_ETHER=y`, but that is a bare menu header and not one of the
  seven drivers under it is selected; `CONFIG_CMD_DHCP` is absent too
  (`DISTRO_DEFAULTS` does not pull it in).  `CONFIG_SMC911X=y` is vestigial
  SMDK5420 lineage - no such chip exists on this board.  For TFTP/DHCP boot add
  `CONFIG_USB_ETHER_RTL8152=y`, `CONFIG_CMD_DHCP=y`, `CONFIG_CMD_PING=y` and set
  `usbethaddr` by hand.  SD boot needs none of it.
- **The MAC address is not guaranteed stable.**  There is no EEPROM on the
  ethernet page of the schematic, and `exynos5422-odroidxu4.dts` has no USB
  device node at all, so there is no `local-mac-address` for the kernel to read
  (the XU3 DTS does have one for its LAN9514).  r8152 falls back to the
  RTL8153's internal `PLA_BACKUP` fuse and then to `eth_hw_addr_random()`.  Most
  units have a fused Hardkernel-OUI address; some do not.  `smsc95xx.macaddr=`
  in Hardkernel's and Armbian's `boot.ini` is a no-op here - wrong chip, and
  mainline r8152 has no module parameters.  Pin it with a systemd `.link` file
  carrying `MACAddress=` and `MACAddressPolicy=none` if DHCP reservations
  matter.
- **`rtl_nic/rtl8153a-3.fw` is not shipped.**  r8152 logs a firmware load
  failure and works anyway - the blob is a PHY/MCU patch, not a requirement.
  Add `FIRMWARE_REPO` plus a `rtl_nic` entry if the errata matter enough to
  justify cloning linux-firmware for one file.
- **`EXYNOS5422_DMC=y` is a tradeoff, not a free win.**  It probes cleanly (the
  `IRQ drex_0 not found` messages are benign; the driver falls back to devfreq
  polling, and IRQ mode has defaulted off since 2020), but the mainline OPP
  table tops out at **825 MHz** where the bootloader leaves DRAM at 933 MHz.
  Armbian compensates with `setenv ddr_freq 825`; postmarketOS sets
  `# CONFIG_EXYNOS5422_DMC is not set` outright.  Turn it off to keep the
  bootloader's clock.
- **Spectre-v2 is permanently unmitigated on the A15 cluster.**  `CPU4..7:
  Spectre v2: firmware did not set auxiliary control register IBE bit, system
  vulnerable`.  Setting IBE is the secure firmware's job and the TrustZone blob
  is Hardkernel's.  Nothing to do about it.
- **2.0 GHz on a low-binned part.**  `exynos5422-asv.c` rewrites per-OPP
  voltages from the SoC's ASV fuses, but `exynos_asv_update_cpu_opps()` only
  adjusts OPPs already in DT and never removes any.  On a part with the
  `EXYNOS5422_BIN2` fuse the ASV table starts at 1800 MHz, so the DT's 1900 and
  2000 MHz entries keep generic voltages and stay selectable.  No distro
  caps the frequency and there are no mainline hang reports, but that is the
  mechanism if one turns up.

## GPU

The Gentoo wiki page for this board says the Mali-T628 "needs external binary
driver".  That is a 2022 statement and it is no longer true.

Mali-T628 is Midgard, GPU ID `0x620` (the same ID as T620 - "T628" is the MP6
count, not a different core).  Mainline binds it directly:
`drivers/gpu/drm/panfrost/panfrost_drv.c` lists `arm,mali-t628` in `dt_match`,
and `panfrost_gpu.c` has `GPU_MODEL(t620, 0x620, ...)`.  The DT node is real and
enabled: `exynos5420.dtsi` declares `gpu@11800000` with
`compatible = "samsung,exynos5420-mali", "arm,mali-t628"`, an OPP table from
177 MHz to 600 MHz and `#cooling-cells`, and `exynos5422-odroid-core.dtsi`
enables it with `mali-supply = <&buck4_reg>`.  It is also wired as a thermal
cooling device off `tmu_gpu`.

Userspace is Mesa's `panfrost` gallium driver, which still carries Midgard v4:
`src/panfrost/model/pan_model.c` has `MIDGARD_MODEL(0x620, "T620", "T62x", ...)`
and `PAN_ARCH 4` is in `panfrost_versions` in every `meson.build`.  It is the
only quirk-free v4 entry - it keeps hierarchical tiling and 8x MSAA, unlike
T600 and T720.  Nothing is being dropped: the Midgard compiler had fixes as
recently as August 2026, and `deqp-panfrost-t720.toml` runs the full GLES2
mustpass in CI (12 fails, 21 flakes, 0 skips).  The catch is that Midgard CI is
`arm-nightly` and manual, not pre-merge gating, so regressions land and get
caught late.

Scanout goes through `exynos-drm` via kmsro, which needs no configuration at
all any more.  `meson.build` sets `with_gallium_kmsro` for any DRM gallium
driver, `pipe_loader_drm.c` opens the exynos fd, finds no gallium descriptor
for `"exynos"` and falls back to kmsro, and `kmsro_drm_winsys.c` allocates
scanout as KMS dumb buffers on the exynos node and imports them into panfrost.
**Never pass `-Dgallium-drivers=kmsro`** - it was removed in Mesa 24.2 and is
now a hard configure error.  Xorg wants `modesetting` + glamor;
`xf86-video-armsoc` is a DRI2/blob-era driver, dead since 2016.

No `kbase`, no `libmali`.  The vendor blobs could not be used on a mainline
kernel even if you wanted them: there is no `drivers/gpu/arm/` in mainline at
all, so there is no `/dev/mali0` for the blob to open.  Hardkernel enabled
panfrost in their own tree in 2024, and no libmali packaging has ever shipped a
T62x variant.

`target-packages.txt` pulls `media-libs/mesa`; `pre-deps.sh` sets
`VIDEO_CARDS="panfrost"` in the crossdev prefix, which the ebuild turns into
`-Dgallium-drivers=...,panfrost`.  It also drags in `dev-util/mesa_clc`, a
native tool that compiles panfrost's OpenCL helper kernels, which is why
`pre-deps.sh` repeats the card selection on the host.  `media-libs/mesa` is
`~arm` on every version, never stable; the target make.conf already sets
`ACCEPT_KEYWORDS="~arm"`, so nothing extra is needed.

### What you actually get

GLES 2.0 and desktop GL 2.1, non-conformant, and that is a hard ceiling, not a
work-in-progress.  Midgard v4 uses SFBD, so `pan_get_max_cbufs()` returns 1 for
`arch < 5`; `mesa/main/version.c` requires `MaxColorAttachments >= 4` for both
GL 3.0 and GLES 3.0.  One colour attachment means no MRT means no ES3.  Ignore
`essl_feature_level = 310` in `pan_screen.c`; MRT overrides it.  No Vulkan
either - `src/panfrost/vulkan/meson.build` is `foreach arch : [6, 7, 10, 12,
13, 14]`, so panvk does not exist for Midgard at all.

Also: T628 MP6 is two core groups of 4 + 2, and panfrost powers on only the
first.  `panfrost_get_core_mask()` says so in a comment, and dmesg will print
`using only 1st core group (4 cores from 6)`.  That was a deliberate 2022
decision to make T628 look like every other Midgard; using group 1 needs a UABI
change nobody has picked up.  `GL_RENDERER` will read `T620 MC6 (Panfrost)` -
T620, not T628, because that is the product ID.

So against the blob this is a real regression: GLES 3.1 + OpenCL 1.1 + 6 cores
becomes GLES 2.0 + no OpenCL + 4 cores.  That is the price of a mainline
kernel, and it is worth stating plainly rather than pretending panfrost is a
drop-in.

## CFLAGS

```
-O2 -march=armv7ve -mtune=cortex-a15.cortex-a7 -mfpu=neon-vfpv4 -mfloat-abi=hard -pipe
```

The A15 and the A7 have the same ISA here - GCC's `arm-cpus.in` gives both
`architecture armv7ve+simd` - so there is no lowest common denominator to fall
back to.  `armv7ve` over plain `armv7-a` buys hardware integer divide in both
ARM and Thumb state.  `-mtune=cortex-a15.cortex-a7` is GCC's big.LITTLE tuning
pair: schedule for the A7, cost like the A15.

`-mfpu=neon-vfpv4` is not optional.  Gentoo's `toolchain.eclass` configures gcc
for an `armv7*-...-gnueabihf` CTARGET with `--with-fpu=vfpv3-d16`, and GCC's
`OPTION_DEFAULT_SPECS` only suppresses that default when an explicit `-mfpu=`
is on the command line - `-march=...+simd` does not suppress it.  Leave it out
and the whole system builds without NEON.

`-O2` rather than the `-O3` the other boards use: 2 GB of RAM and a 32-bit
address space make `-O3`'s code growth a bad trade here.

This is a deviation from the stage3, which is built `-march=armv7-a` with the
vfpv3-d16 default.  Everything built here is a strict superset and runs on the
board, but the binpkgs are not interchangeable with Gentoo's official
`arm/binpackages/23.0/armv7a_hf` binhost.

## Gentoo target

| | |
|---|---|
| `BOARD_ARCH` | `armv7a` |
| CHOST | `armv7a-unknown-linux-gnueabihf` |
| Profile | `default/linux/arm/23.0/armv7a_hf` (stable) |
| stage3 | `stage3-armv7a_hardfp-openrc` |
| Gentoo ARCH | `arm` |

Note the profile path: 23.0 renamed the old 17.0 `armv7a/hardfloat` to
`armv7a_hf`.  The stage3 flavour kept the old `armv7a_hardfp` name, so the two
do not match, and the mirror directory is `releases/arm/` for every 32-bit ARM
subarch.  `armv7a_hardfp-openrc` is still built weekly.

## What has been seen on hardware

An image from this board directory has booted an Odroid-XU4 to a login prompt
on the serial console.  That is the first armv7a boot in this tree, and it
settles three things that were guesses when this was written:

- Mainline U-Boot v2026.07 fits the 720 KiB window BL2 gives it.
  `post-bootloader.sh` fails loudly if it ever stops fitting; the fix would be
  to drop `CMD_THOR_DOWNLOAD`, `CMD_DFU` and `USB_GADGET`, none of which SD
  boot needs.
- Truncating BL1 to 15360 bytes produces a BL1 the iROM accepts.
- The board reaches userspace from the SD card.  The root device is named by
  PARTUUID rather than a path, so it does not depend on which slot the card
  lands in -- an earlier revision of this file said `/dev/mmcblk1p2`, which was
  wrong twice over (the SD slot is mmc2, and the path form was replaced).

HDMI output works on a monitor.  A blank screen through an MS2130 USB capture
card was the capture card, not the board: every layer below it -- kernel
config, device tree, blanking, fbcon and the mixer registers including the
shadow bank -- was checked and is correct.

Not yet seen: no image built after the serial getty was fixed has been booted.
The board used to print `INIT: Id "s0" respawning too fast` and repeat the
login banner down the screen, because its getty ran without `-L` on a debug
header that has no modem control lines.  That is fixed in the framework, and
the fix is in the current image, but the current image has not been on the
board.

There is no serial log committed under `evidence/` yet, so nothing here can be
checked by anything but a person reading it.

## Still not verified
- **Mesa on 32-bit ARM.**  `media-libs/mesa` is unversioned in
  `target-packages.txt`, which means whatever is newest.  Two open upstream bugs
  say that is optimistic:
  - [mesa#13236](https://gitlab.freedesktop.org/mesa/mesa/-/issues/13236),
    "Mesa 25 breaks on 32-bit ARM", reported on exactly this GPU
    (`ARM Mali-T628 MP6`, Exynos 5420, `armv7l`): GNOME/mutter will not start,
    worked on Mesa 24.  Open, 54 comments, last touched March 2026.  Looks like
    a 32-bit linkage problem from the shared-glapi/libgallium consolidation
    rather than anything panfrost-specific.
  - [mesa#16052](https://gitlab.freedesktop.org/mesa/mesa/-/issues/16052),
    Midgard rendering artifacts in Firefox, a 26.2 regression against 26.1.6.
    Opened August 2026, still open.

  If the GL stack misbehaves, pin `~media-libs/mesa-26.1.7` before debugging
  anything else.  Cross-emerging mesa for armv7a is unproven here, but Alpine
  builds panfrost for `armhf` and `armv7`, so the path exists.  Dropping the
  `media-libs/mesa` line still leaves a bootable image with the kernel-side GPU
  driver in place.
