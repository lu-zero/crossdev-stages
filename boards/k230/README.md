# Canaan K230 (CanMV-K230-V1.1, 01Studio CanMV K230)

Dual-core RISC-V: C908 (vector) + C906 (no vector), heterogeneous.

## Boot chain

```
Boot ROM -> U-Boot SPL (fn_u-boot-spl.bin) -> U-Boot -> OpenSBI (fw_payload + kernel) -> Linux
```

OpenSBI embeds the kernel as `FW_PAYLOAD`. `BUILD_STEPS` puts kernel before bootloader.

## Dependencies

Build requires `xxd`, `dd`, `sha256sum` (for firmware header), `mkimage` (for uImage/vmlinuz packaging).

## Firmware header

`make-k230-firmware.sh` wraps U-Boot SPL into K230 boot ROM format:
532-byte header (magic "K230" + length + SHA-256 + padding) + payload.

## References

- https://dev.to/andelf/bare-metal-embedded-programming-on-k230-using-rust-4h4g
- https://code.videolan.org/Courmisch/k230-boot
- https://github.com/andelf/k230-bare-metal
- https://github.com/revyos/mkimg-k230
