# VLSI Solutions VSRVES01 (VSRV1 RV32IMS Linux core)

VLSI Solution's first Linux-capable RISC-V SoC — and our first **rv32**
board and first **non-OpenSBI / non-u-boot** boot pipeline.  Dev kit:
the **CAT Board** (QFN-88 chip + LPDDR2 ≥128 MiB + SPI flash + microSD
+ 10/100 RGMII Ethernet + 16550 UARTs + stereo audio).

## ISA

`riscv,isa` = RV32IMS + zicsr + zifencei, **soft-float** (no F/D), sv32 MMU.

GCC target: `-march=rv32ima_zicsr_zifencei -mabi=ilp32 -mcmodel=medlow`
(vendor toolchain config matches; `S` is the privileged spec and is
implicit in any kernel build).  `-Os` chosen because the chip ships with
128 MiB LPDDR2 — every byte matters.

Gentoo profile: `default/linux/riscv/23.0/rv32/ilp32` (glibc/openrc),
stage3 variant `rv32_ilp32-openrc`.  Both verified in autobuilds.

## Boot chain (qualitatively different from every other board here)

```
SPI Flash
  -> VSDSP6 (proprietary VLSI DSP core) loads VSOS at power-on (~1.5s)
  -> VSOS reads `startup.txt` from microSD FAT32 root
  -> `ddrload -brvlbne.bin -B -bcatboard.dtb newlinux.vri`
         |
         |  rvlbne.bin loaded at 0x80000000  (tiny RISC-V bootloader stub)
         |  catboard.dtb appended in-RAM with -B (DDRLoad inserts mac/clk/mem)
         |  newlinux.vri unpacked at 0x80400000  (Linux kernel)
         v
  -> VSDSP starts the VSRV1 core
  -> rvlbne -> Linux Image -> catboard.cpio initramfs -> /sbin/init
```

No u-boot, no OpenSBI, no extlinux, no EFI.  The VSDSP side is what we'd
call the "BL1" in ARM TF-A terms, and the boot artefact is VLSI's `VRI`
image format (User's Guide §15.1 — encoded inline in `post-assemble.sh`).

## What this board ships

`image build --board vsrves01` produces `vsrves01-sdcard.img` containing
a single FAT32 partition with:

- `rvlbne.bin`    — VLSI RISC-V bootloader stub (vendor binary)
- `catboard.dtb`  — device tree (vendor binary)
- `linux61.vri`   — our kernel `Image` wrapped as VRI (same filename
  the vendor card uses, so it overrides the preloaded kernel)
- `startup.txt`   — full VSOS shell script (BadBit / SetClock /
  +CATSEYES / +uartin / +rvparam / RvParam / ddrload / !Shell);
  overwrites the vendor's preloaded `startup.txt`

**No on-disk Gentoo userland.**  The kernel embeds `catboard.cpio`
(vendor initramfs) and runs entirely from RAM.  Adding a real userland
(switch_root from initramfs to a SD-mounted Gentoo rootfs) is the
`vsrves01-full` variant for later.

## Flash procedure

1. Use the microSD card that came with your CAT Board — VSOS only
   ships on the vendor card (and in on-chip SPI flash); we cannot
   redistribute it.  Bare-chip buyers cannot boot this build.
2. Copy the contents of our `.img`'s FAT partition onto the card,
   **overwriting** the vendor's `startup.txt` and `linux61.vri`
   (don't reformat — keeps `vsos/`, `README.TXT`, `Welcome.txt` etc.
   intact):
   ```sh
   sudo losetup -fP --show vsrves01-sdcard.img        # /dev/loopN
   sudo mount /dev/loopNp1 /mnt
   sudo cp -a /mnt/. /path/to/sdcard/
   sudo umount /mnt
   sudo losetup -d /dev/loopN
   ```
   Our `startup.txt` replays every command the vendor card runs
   before `ddrload` (BadBit, SetClock -l130 98, +CATSEYES, +uartin,
   +rvparam, RvParam +mba:...), so overwriting is safe.
3. Insert into the CAT Board's microSD slot and power on.  Serial
   console at 115200 8N1 on UART0 (`ttyS0`).
4. At the VSOS prompt (`S:>`), `startup.txt` auto-runs and chains into
   Linux in a few seconds.

## Known mainline gaps

- **No upstream support.** VSRV1 core is not in `arch/riscv/boot/dts/`,
  vendor patches kernel out of tree (one-liner to
  `drivers/clocksource/timer-riscv.c`).
- **Ethernet driver** is an out-of-tree module (`vlsi-lnx-drv`) built
  against the kernel; lives at
  https://www.vsdsp-forum.com/phpbb/viewtopic.php?t=3248 .
- **VSDSP side** (audio DSP) requires VLSI's proprietary toolchain
  (VSIDE / lcc).  Not covered by this board — we only build Linux.
- **VSRVES01 is engineering sample** (datasheet v0.01, "contact us to
  buy").  Production silicon timing TBD.
- **VRI encoder is unverified.**  Our inline Python encoder
  (`post-assemble.sh`) matches User's Guide §15.1 by inspection but
  has not been byte-diffed against vendor `elf2vri` output.  If
  rvlbne refuses to jump, build vendor `elf2vri` from
  <https://www.vsdsp-forum.com/phpbb/viewtopic.php?t=3247> and
  diff one section.

## Prerequisites before build

You must manually download two vendor blobs into `firmware/` before
running `image build` — see `firmware/README.md`.  They aren't in any
git repo and the VLSI forum's terms are too unclear for us to mirror.

## References

- Product page: https://www.vlsi.fi/en/products/vsrves01.html
- CAT Board: https://www.vlsi.fi/en/support/evaluationboards/vsrves01catboard.html
- User's Guide (1.01): https://www.vlsi.fi/fileadmin/products/vsrv/vsrv_guide.pdf
- Datasheet (preliminary v0.01): https://www.vlsi.fi/fileadmin/datasheets/vsrves01_ds.pdf
- VSRV1 core (RTL + FPGA, Tristan/SocHub EU project):
  https://www.vlsi.fi/en/vsrv/vsrv1.html
- DTS forum post: https://www.vsdsp-forum.com/phpbb/viewtopic.php?t=3241
- Ethernet driver: https://www.vsdsp-forum.com/phpbb/viewtopic.php?t=3248
- VRI file format: User's Guide §15.1 (re-implemented in `post-assemble.sh`)
- VSOS Shell: https://www.vlsi.fi/fileadmin/products/vsrv/vsrv_vsos_shell.pdf
