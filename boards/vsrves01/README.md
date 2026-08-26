# VLSI Solutions VSRVES01 (VSRV1 RV32IMS Linux core)

> **Status: EXPERIMENTAL / UNVALIDATED.**  The development board died
> (no power LED) on 2026-06-27, mid-validation.  The boot path was
> verified through OpenRC service startup on the last built image, but
> the final full boot test never ran.  Everything below builds and is
> believed correct; nothing has been re-verified on hardware since.
> Resume notes live in the project memory
> (`project_vsrves01_phram_final`).

VLSI Solution's first Linux-capable RISC-V SoC — and our first **rv32**
board, first **musl** board, and first **non-OpenSBI / non-u-boot**
boot pipeline.  Dev kit: the **CAT Board** (QFN-88 chip + LPDDR2
128 MiB + SPI flash + microSD + 10/100 Ethernet + 16550 UARTs + stereo
audio).

## ISA and toolchain

`riscv,isa` = RV32IMS + zicsr + zifencei, **soft-float** (no F/D),
**no C extension**, sv32 MMU.

GCC target: `-Os -march=rv32ima_zicsr_zifencei -mabi=ilp32
-mcmodel=medlow` (`S` is the privileged spec, implicit in any kernel
build).  `-Os` because the chip ships with 128 MiB LPDDR2.

Gentoo profile: `default/linux/riscv/23.0/rv32/ilp32/musl` — there is
no glibc rv32 stage3, the upstream rv32 profile is the musl one.
CHOST `riscv32-unknown-linux-musl`.  Because stage3 binaries are built
with the profile-default `-march=rv32imac` (WITH the C extension),
`override-kernel.sh` rebuilds the entire @system **from source** with
our CFLAGS (`emerge -e @system --usepkg=n`) — any RVC-tainted binary
traps "Invalid opcode" on this SoC.

## Boot chain (qualitatively different from every other board here)

```
SPI Flash
  -> VSDSP6 (proprietary VLSI DSP core) loads VSOS at power-on (~1.5s)
  -> VSOS reads `startup.txt` from the microSD FAT16 superfloppy
  -> ddrload +v -brvlbne.bin -B -bcatboard.dtb -a0x83000000 -bimage.sqfs linux61.vri
         |
         |  rvlbne.bin  @ 0x80000000  tiny RISC-V bootloader stub (vendor)
         |  catboard.dtb appended in-RAM with -B (DDRLoad patches mac/clk/mem)
         |  image.sqfs  @ 0x83000000  our Gentoo rootfs (80 MiB phram window)
         |  linux61.vri @ 0x80400000  our kernel (VRI internal addressing)
         v
  -> VSDSP starts the VSRV1 core
  -> rvlbne -> Linux -> phram mounts /dev/mtdblock0 (squashfs) -> /sbin/init
```

No u-boot, no OpenSBI, no extlinux, no EFI, no initramfs.  The VSDSP
side is what we'd call the "BL1" in ARM TF-A terms, and the boot
artefact is VLSI's `VRI` image format, produced by the `bin2vri` C
hostprog our kernel patches add (`make Image.vri`, patches 0009/0010).

## phram-rootfs architecture

The SD bus is physically owned by the VSDSP6 DSP — Linux cannot reach
the card.  Instead of the vendor's initramfs-only approach, the full
Gentoo userland is a squashfs (`image.sqfs`, lz4) that DDRLoad places
in DDR at boot; the kernel maps it with `CONFIG_MTD_PHRAM` and mounts
it read-only as the root filesystem.  DDR budget (128 MiB):

```
0x80000000-0x803fffff  vendor blobs (rvlbne + _ddrBlob + dtb)  [boot only]
0x80400000-~0x80a00000 kernel (~6.4 MiB, no cpio embed)
     ...   -0x82ffffff Linux-managed RAM (~44 MiB)
0x83000000-0x87ffffff  phram window (80 MiB) = image.sqfs
```

The phram window is protected from the kernel allocator by a
`/reserved-memory` node (no-map) that `override-assemble.sh` writes
into `catboard.dtb` with `fdtput`, alongside the `/chosen/bootargs`:

```
console=ttyS0,115200 console=ttyUL0,1000000
phram.phram=rootfs,0x83000000,0x5000000
root=/dev/mtdblock0 rootfstype=squashfs ro rootwait
```

(`phram.phram=` because the driver is built in; ttyUL0 last so it
becomes `/dev/console`.)

## Kernel

Mainline `v6.18.36` + the 11-patch in-tree port of esmil/vsrv-linux
under `patches/`: dt-bindings + DTS (0001-0004), the in-tree VLSI
Ethernet MAC driver (0005/0006 — replaces the vendor's out-of-tree
`vlsi-lnx-drv` module), clocksource min-delta fix (0007), SBI stub
(0008 — there is no SBI firmware), the `bin2vri` host tool and
`Image.vri` make target (0009/0010), and `vsrves01_defconfig` (0011:
MTD_PHRAM, SQUASHFS_LZ4, no BLK_DEV_INITRD).

## What this board ships

`image build --board vsrves01` produces a **FAT16 superfloppy** image
(no MBR/GPT — the VSDSP6 FAT driver wants FAT16 and reads sector 0 as
a BPB) containing:

- `linux61.vri`   — our kernel wrapped as VRI (same filename the
  vendor card uses, so it overrides the preloaded kernel)
- `catboard.dtb`  — built from our in-tree DTS, `/chosen/bootargs` +
  `/reserved-memory` injected via fdtput
- `image.sqfs`    — the Gentoo rootfs squashfs (musl, OpenRC,
  dropbear, busybox udhcpc; see `target-packages.txt`)
- `rvlbne.bin`, `rvparam.bin`, `shell.ap3`, `SYSR/*.dr3`, … — vendor
  VSOS payloads staged from the firmware zip
- `startup.txt`   — the **vendor** file with two `sed` edits applied
  by `override-assemble.sh`: uncomment `Term -a` (forward the VSDSP
  UART to the RV side) and extend the `ddrload` line with
  `-a0x83000000 -bimage.sqfs` so the rootfs lands in the phram window

## Flash procedure

1. Use the microSD card that came with your CAT Board — VSOS only
   ships on the vendor card (and in on-chip SPI flash); we cannot
   redistribute it.  Bare-chip buyers cannot boot this build.
2. Copy the contents of our image's FAT filesystem onto the card,
   **overwriting** the vendor's `startup.txt` and `linux61.vri`
   (don't reformat — keeps `vsos/` etc. intact):
   ```sh
   unxz gentoo-linux-vsrves01_dev-sdcard-<ts>.img.xz
   sudo losetup -f --show gentoo-linux-vsrves01_dev-sdcard-<ts>.img  # /dev/loopN
   sudo mount /dev/loopN /mnt          # superfloppy: no partitions
   sudo cp -a /mnt/. /path/to/sdcard/
   sudo umount /mnt
   sudo losetup -d /dev/loopN
   ```
3. Insert into the CAT Board's microSD slot and power on.  The VSOS
   prompt (`S:>`) talks at 115200 8N1; once Linux is up the console is
   `ttyUL0` at **1000000** baud on the same cable (VSOS `Term -a`
   forwards it) — switch your terminal's baud rate accordingly.
4. `startup.txt` auto-runs and chains into Linux; expect the OpenRC
   login prompt after ~2 minutes (98 MHz core).

## Prerequisites before build

You must manually download the vendor firmware zip into `firmware/`
before running `image build` — see `firmware/README.md`.  It isn't in
any git repo and the VLSI forum's terms are too unclear for us to
mirror.

## Known gaps

- **Hardware validation incomplete** — see the status note at the top.
- **VSDSP side** (audio DSP) requires VLSI's proprietary toolchain
  (VSIDE / lcc).  Not covered by this board — we only build Linux.
- **VSRVES01 is engineering sample** (datasheet v0.01, "contact us to
  buy").  Production silicon timing TBD.
- **Patches are a port, not upstream.**  The DTS/driver set under
  `patches/` is our v6.18.36 port of esmil/vsrv-linux; nothing is in
  mainline yet.

## References

- Product page: https://www.vlsi.fi/en/products/vsrves01.html
- CAT Board: https://www.vlsi.fi/en/support/evaluationboards/vsrves01catboard.html
- User's Guide (1.01): https://www.vlsi.fi/fileadmin/products/vsrv/vsrv_guide.pdf
  (DDRLoad `-a`/`-b` semantics §6.1.9, VRI format §15.1)
- Datasheet (preliminary v0.01): https://www.vlsi.fi/fileadmin/datasheets/vsrves01_ds.pdf
- VSRV1 core (RTL + FPGA, Tristan/SocHub EU project):
  https://www.vlsi.fi/en/vsrv/vsrv1.html
- Original kernel port: https://github.com/esmil/vsrv-linux
- DTS forum post: https://www.vsdsp-forum.com/phpbb/viewtopic.php?t=3241
- VSOS Shell: https://www.vlsi.fi/fileadmin/products/vsrv/vsrv_vsos_shell.pdf
