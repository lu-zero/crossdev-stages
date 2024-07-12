# crossdev-stages
Build Gentoo stages leveraging crossdev

## Status

- [x] Build and assemble packages to a stage1 [catalyst](https://wiki.gentoo.org/wiki/Catalyst) can leverage
- [x] Update a compatible stage3 image
- [x] Build opensbi + u-boot images and linux kernel + modules
- [ ] Assemble bootable images

## Platforms
- riscv64 (bpi-f3)


## Dependencies
``` sh
# Needed to build all the stages
emerge crossdev merge-usr git
# Needed to build the bootloader and kernel
emerge u-boot-tools dtc
# Needed to investigate the image and run ldconfig
emerge bubblewrap qemu
```

## Limitations

- Some packages are cross-compilation unfriendly and rely on runtime checks (e.g. perl modules)
