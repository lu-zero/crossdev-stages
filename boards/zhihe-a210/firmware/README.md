# Zhihe A210 closed firmware blobs

These three blobs are required for full A210 functionality.  They are
**not** redistributed by the OSU Open Source Lab vendor-source mirrors
and **not** committed to this repo — per the vendor buildroot README
they are redistributable *inside* a vendor-built image but not standalone.

Drop them into this directory before building:

```
boards/zhihe-a210/firmware/
├── a210-aon.bin         # ~52K — E902 AON core firmware (PMIC, RTC, reboot, regulators)
├── bootzero-rvbl.bin    # early DDR init, fastboot stage 1
└── bootzero2.bin        # fastboot stage 1.5 (BTZ-with-SPL chain)
```

## Where to get them

Download the Zhihe SDK release archive:

```
https://developer.zhcomputing.com/downloads/release/zhihesdk/v2.8.1/
```

Unpack and copy:

- `a210-aon.bin` from `vendor/firmware/` (or extract from any released
  vendor image's `/lib/firmware/zhihe/`).
- `bootzero-rvbl.bin`, `bootzero2.bin` from `bootzero/` or `prebuilts/`.

## What happens without them

- **a210-aon.bin**: kernel boots, but `reboot`/`poweroff` hang and some
  regulators (touch, audio, charger) stay off.  `override-kernel.sh`
  falls back to "look for it at runtime" mode and prints a warning.
- **bootzero-rvbl.bin**: only matters for first-time bring-up on a
  bare board.  Once vendor u-boot is in eMMC mmcblk0boot0, subsequent
  flashes go through u-boot's own fastboot and skip stage 1.
- **bootzero2.bin**: used only by the `btz-with-spl-rvbl.bin` chain
  for full chip programming; our flash.sh does not require it.

## Vendor source reference

The exact RVBL header format + wrap scripts live in vendor source at:

```
zhihe-a210-u-boot/board/zhihe/common/script/generate_firmware.sh
```

Our `post-assemble.sh` re-implements `generate_rvbl()` in inline Python
so we don't need to vendor the shell script.
