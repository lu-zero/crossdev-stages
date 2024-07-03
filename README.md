# crossdev-stages
Build Gentoo stages leveraging crossdev

## Status

- [x] Build and assemble packages to a stage1 [catalyst](https://wiki.gentoo.org/wiki/Catalyst) can leverage
- [ ] Update a compatible stage3 image
- [ ] Assemble bootable images

## Platforms
- riscv64 (bpi-f3)

## Limitations

- Some packages are cross-compilation unfriendly and rely on runtime checks (e.g. perl modules)
