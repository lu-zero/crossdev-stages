# Orange Pi 5 Ultra (RK3588)

Xunlong Orange Pi 5 Ultra: RK3588 (4x Cortex-A76 + 4x Cortex-A55), Mali-G610
MP4, 6 TOPS RKNN NPU, LPDDR5.  Everything below is upstream: no vendor kernel,
no vendor U-Boot.

## Boot chain

| Stage | Source | Blob? |
|---|---|---|
| TPL (DDR init) | `rockchip-linux/rkbin` | yes, the only closed piece |
| BL31 | upstream ARM Trusted Firmware, `PLAT=rk3588` | no |
| U-Boot | mainline, `orangepi-5-ultra-rk3588_defconfig` | no |
| Linux | mainline, `arch/arm64/.../rk3588-orangepi-5-ultra.dts` | no |

U-Boot boots via extlinux from the ext4 boot partition.  Serial console is
uart2m0 (3-pin header) at 1500000 baud; SD card is `mmcblk1`, the eMMC socket
is `mmcblk0`.

## Accelerators

RK3588 mainline support is largely Collabora's work, and all of it is reachable
from `defconfig` plus the three symbols set in `override-kernel.sh`:

| Block | Driver | Config | In defconfig |
|---|---|---|---|
| NPU (3x RKNN cores) | `accel/rocket` | `DRM_ACCEL_ROCKET=m` | no, enabled here |
| H.264/HEVC decode | `rockchip-vdec` (vdpu381) | `VIDEO_ROCKCHIP_VDEC=m` | no, enabled here |
| GPU (Mali-G610) | `panthor` | `DRM_PANTHOR=m` | yes |
| AV1 decode | `hantro` + `vsi-iommu` | `VIDEO_HANTRO=m`, `VSI_IOMMU=y` | hantro yes, IOMMU no |
| JPEG encode | `hantro` (`vepu121`) | `VIDEO_HANTRO=m` | yes |
| Display / HDMI | `rockchip-drm` + `dw-hdmi-qp` | `DRM_ROCKCHIP=m` | yes |
| CPU DVFS / thermal | SCMI cpufreq, `rockchip-thermal` | `ARM_SCMI_CPUFREQ=y` | yes |

`av1d` in `rk3588-base.dtsi` sits behind `av1d_mmu`, whose
`verisilicon,iommu-1.2` is driven by `VSI_IOMMU`.  Nothing in the tree selects
that symbol and defconfig does not carry it, so the decoder would otherwise
probe with its IOMMU missing.

The NPU needs nothing board-specific: `rk3588-orangepi-5.dtsi` already sets
`npu-supply`/`sram-supply` and `status = "okay"` on all three `rknn_core_*`
nodes and their IOMMUs, so a mainline DTB is enough.  Check after boot:

```
ls /dev/accel/          # accel0
dmesg | grep -e rocket -e panthor
```

The kernel command line carries `cma=256M`.  The 160 MiB HDMI-RX pool is a
separate reservation; the global CMA default of 32 MiB is what the VPU and the
GPU allocate from, and 32 MiB does not cover 4K buffers.

## Firmware

Pulled from upstream linux-firmware (`FIRMWARE_REPO`, pinned by
`FIRMWARE_TAG`); `FIRMWARE_DIRS` lists the directories, copied with their
paths intact.  One entry per part the DT actually describes:

| Directory | Part | Why |
|---|---|---|
| `arm/mali` | Mali-G610 (`arm,mali-valhall-csf`) | panthor requests `arch10.8/mali_csffw.bin` and fails to probe without it |
| `rtl_nic` | Realtek NIC behind `pcie2x1l1` | r8169 requests `rtl_nic/rtl81*.fw` |

The VPU and the NPU need no firmware.  The RGMII PHY does not either.  For a
USB or M.2 device, add its directory to `FIRMWARE_DIRS`; that is one line,
and it beats shipping the multi-GB tree whose bulk is amdgpu/nvidia/intel.

## The AP6611S is not usable here

The board carries an AP6611S (Synaptics SYN43711) on SDIO for Wi-Fi and on
uart7 for Bluetooth.  Neither half works from upstream alone:

- Wi-Fi has no mainline driver.  `sdio_ids.h` stops at 43752 and brcmfmac has
  no ID for 43711, so this image cannot bring the radio up at all.  The vendor
  path is the Android `bcmdhd` driver, which the BSP patched to sit on
  Rockchip's `rfkill-wlan`, so it does not port to a newer kernel either.
  The upstream-shaped fix is brcmfmac support (the part is SDIO FullMac, like
  the AP6275s that brcmfmac already handles); an out-of-tree attempt gets as
  far as associating without passing data.
- Bluetooth binds (the DT node is upstream) but its patch file `BCM4362A2.hcd`
  is not in linux-firmware, which ships only two `.hcd` files.  btbcm treats a
  missing patch as non-fatal, so expect a working-but-unpatched controller
  unless you supply the vendor blob yourself.

Use a USB radio.  Pick one by its client limit: mt7612u handles 128 stations,
while mt7921 caps at 15, which is the wrong number for a room full of people.

## Userspace

- GPU: `media-libs/mesa` with `VIDEO_CARDS="panfrost"` (set by `pre-deps.sh`)
  gives panfrost GL and panvk Vulkan.  Mesa itself only *build*-depends on
  meson, so cross-emerge should work; installing meson itself into an image
  is the fragile case, and this board does not.
- NPU: Mesa's `rocket` gallium driver plus the TensorFlow Lite delegate
  (`-Dgallium-drivers=rocket -Dteflon=true`).  Gentoo's ebuild exposes neither
  option, so this needs a local ebuild.
- Video playback: GStreamer needs 1.28+ for the RK3588 stateless decoder path,
  and ::gentoo is still on 1.26.  FFmpeg/mpv have no V4L2 Request support
  upstream at all, so both need out-of-tree patches.  So the decoder works from
  a bare `v4l2-ctl` today, but a full playback stack does not come from
  ::gentoo yet.

## CFLAGS

`-mcpu=cortex-a76.cortex-a55+crc+crypto` is the big.LITTLE tuning pair, same as
`odroid-m2`.  Both clusters implement the crypto extensions.
