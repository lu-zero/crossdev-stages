# CLI Subcommand Roadmap

## Current

```
crossdev-stages
  sandbox   setup|list|prepare|crossdev|enter|run|destroy
  target    setup|list|stage1|update|install|ldconfig|destroy|export
  image     build|prune|export
  stages    list|fetch
  board     list|info
  maint     cleanup|logs|doctor
  status
```

## TODO

### CLI
- [ ] `enter <board>` -- shell with target stage + build mounted
- [ ] `maint cache list|size|clean` -- PKGDIR binary package management
- [ ] `sandbox clone` -- cp -al for parallel builds
- [ ] `logs --follow` -- tail -f style
- [ ] `export --format raw` -- decompress before export
- [ ] `image build --parallel` -- deps sequential, rest parallel

### Architecture
- [ ] Library conversion (lib.rs) -- thin CLI wrapper over pub API
- [ ] Workspace::at(path) -- custom workspace path for CI
- [ ] Default package lists -- common base + per-board additions
- [ ] rkbin DDR filename auto-detect -- glob instead of hardcoded path

### Done
- [x] logs, export, config, doctor, cleanup
- [x] Bootloader modularize (opensbi.rs, uboot.rs)
- [x] Hook convention (pre/post/override-{step}.sh)
- [x] Source cache (bare repo references)
- [x] Build timing per step
- [x] Odroid M2 board config (aarch64)
- [x] K230 firmware.py -> bash
- [x] Smart bootloader defaults (OPENSBI_FW_TYPE, MAKE_FLAGS)
- [x] pyelftools + pkg-resources host deps
