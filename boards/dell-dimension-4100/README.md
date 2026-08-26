# Dell Dimension 4100 (Pentium III Coppermine, i815E)

Retail Socket 370 desktop, 2000 vintage.  Pentium III Coppermine at
866/933/1000 MHz (MMX and SSE, no SSE2), Intel 815E chipset (FW82815 GMCH
plus FW82801BA ICH2), up to 512 MB PC133, one serial port, two USB 1.1
ports, Ultra ATA disk, Phoenix BIOS (newest A11).  Dell's OEM build of
Intel's D815EEA; the UART is on an SMSC LPC47M102 Super I/O.

Nothing here has been booted on the hardware.  Every claim below comes
from the kernel tree, the chipset datasheets and Dell's archived tech
specs, not from a serial log.

## Boot chain

No firmware stage of any kind: no U-Boot, no OpenSBI, no vendor blob.

```
BIOS (Phoenix A11, on-board flash)
  -> MBR boot code = GRUB boot.img, patched at byte 92 with the LBA of core.img
  -> core.img in the gap at LBA 1, prefix (hd0,msdos1)/grub
  -> /grub/grub.cfg on the FAT32 boot partition
  -> /bzImage
```

`grub-mkimage` runs in the sandbox; the i386-pc modules come from the
crossdev prefix, built by the same portage package version as the host
tool, so the module ABI matches.  `genimage.cfg` writes only the first 440
bytes of `boot.img` so the disk signature and partition table survive.

## Disk layout

MBR, 512-byte sectors.

| Offset | Size | Contents |
|---|---|---|
| 0 | 440 B | GRUB `boot.img` |
| 440 | 4 B | disk signature, half of the root `PARTUUID` |
| 512 | up to 1 MiB | GRUB `core.img` |
| 1 MiB | 128 MiB | boot partition, FAT32, type 0x0c, bootable |
| 129 MiB | rest | root partition, ext4, type 0x83 |

Everything is inside the first 8.4 GB, so it is reachable whether the BIOS
answers the INT 13h LBA extension check or falls back to CHS.

Root is named `PARTUUID=${BOOT_DISK_ID}-02`, not `/dev/sda2`: the disk is
`sda` only while it is master on the primary IDE channel, and this machine
has two channels and a CD-ROM.  `root=LABEL=` cannot be used at all, with
or without a device path: `early_lookup_bdev()` in `block/early-lookup.c`
takes only `PARTUUID=`, `PARTLABEL=`, `/dev/` and a raw device number, and
`PARTLABEL` is GPT-only.  There is no initramfs to resolve anything else.

## Kernel

Mainline v7.2, `i386_defconfig` plus two fragments.

`kernel-config/pentium3` sets `CONFIG_MPENTIUMIII=y`.  The defconfig names
no processor family, so the `arch/x86/Kconfig.cpu` choice defaults to
`M686`; MPENTIUMIII adds `-mtune=pentium3` and turns on
`X86_INTEL_USERCOPY`.

`kernel-config/dimension-4100` adds `CONFIG_SND_INTEL8X0=y` (8086:2445,
the ICH2 AC97 controller behind Dell's "SoundMAX 2.0") and
`CONFIG_VORTEX=y` (the 3C905C-TXM Dell listed as an option), and turns
`CONFIG_DRM_I915` off.

The defconfig already has ATA_PIIX, BLK_DEV_SD, E100, SERIAL_8250 with
its console, USB_UHCI_HCD, and EXT4_FS and VFAT_FS built in, which is what
lets the kernel mount root with no initramfs.

## Compiler flags

`-O2 -march=pentium3 -pipe`, arch `i686`.

Coppermine has MMX and SSE and no SSE2.  `-march=pentium3` is exactly that
ISA; every `-march` above it turns SSE2 on and an SSE2 instruction faults
on this CPU.  `-mfpmath` is left at its 387 default on purpose, because
SSE1 has no double precision.  `i686` rather than `i586` so that Gentoo's
`default/linux/x86/23.0/i686` profile, whose `CPU_FLAGS_X86="mmx sse"`,
tells ebuilds the same thing the compiler is emitting.

## Write the image

```sh
xzcat gentoo-linux-dell-dimension-4100_dev-disk-<timestamp>.img.xz \
    | sudo dd of=/dev/<target> bs=4M status=progress conv=fsync
```

Write it to the PATA disk in a USB enclosure or on another machine's IDE
channel, then put it back as master on the primary channel.  The root
partition grows to the end of the disk on first boot (`grow-rootfs`).

## Console

Serial is COM1, `ttyS0` at 115200 8N1, and it is the last `console=` on
the command line, so it owns `/dev/console`.  `console=tty0` comes first,
so the monitor sees the boot messages too.

## Known not to work

- **Integrated video is unaccelerated VGA text.**  Nothing in this tree
  binds the 815's GMCH: i915's `pciidlist` starts at `INTEL_I830_IDS`,
  `INTEL_I815_IDS` (0x1132) is defined in `include/drm/intel/pciids.h` and
  referenced nowhere under `drivers/`, and the legacy `drm/i810` driver is
  gone.  `CONFIG_FB_I810` still exists but does not build as shipped:
  `i810_accel.c` calls `cfb_fillrect`, `cfb_copyarea` and `cfb_imageblit`
  while its Kconfig entry selects `FB_IOMEM_FOPS`, which pulls in none of
  them, instead of `FB_IOMEM_HELPERS`, which selects all three.  That looks
  like an upstream Kconfig bug.
- **Which NIC is fitted is unknown.**  Both drivers are built in, the
  integrated Intel 10/100 (`e100`) and the optional 3C905C-TXM
  (`3c59x`).  One of them is dead weight on any given unit.
- **ACPI 1.0 and an SMP kernel with LOCAL_APIC/IO_APIC.**  Both are common
  sources of hangs on 2000-era firmware.  The second GRUB entry boots with
  `acpi=off noapic nolapic`; if the machine needs it every time, that
  belongs in the first entry.
- **The RTC battery is flat.**  `ntpd` is enabled rather than
  `ntp-client`, which would run before dhcpcd has a lease and give up.

## Default credentials

- root, empty password (development image, change on first login)
- sshd enabled, `PermitRootLogin yes`, `PermitEmptyPasswords yes`
