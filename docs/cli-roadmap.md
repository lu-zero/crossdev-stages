# CLI Subcommand Roadmap

## Current

```
crossdev-stages
  sandbox   setup|list|prepare|crossdev|enter|run|destroy
  target    setup|list|stage1|update|install|ldconfig|destroy
  sysroot   list|create|destroy
  image     list-boards|build|prune
  stages    list|fetch
  cleanup   [--all] [--dry-run]
```

## Planned

### `logs` -- view build logs (like `docker logs`)
```
crossdev-stages logs <board>               # latest build log
crossdev-stages logs <board> --step <step> # specific step
crossdev-stages logs <board> --follow      # tail -f
crossdev-stages logs --list                # list available logs
```
Requires: per-step log capture in run_step(), store in build dir.

### `export` -- export build artifacts
```
crossdev-stages export <board>                    # export latest image
crossdev-stages export <board> --output /path/    # to specific dir
crossdev-stages export <board> --format raw       # decompress (no xz)
crossdev-stages export <board> --step bootloader  # export specific artifact (idbloader.img, u-boot.itb)
```
Skips internal files (.board, .done-* markers). Copies only the useful output.

### `status` -- show build/sysroot/sandbox state
```
crossdev-stages status                   # overview of everything
crossdev-stages status <board>           # board-specific: sysroot, latest build, steps done
```

### `config` -- show resolved board config
```
crossdev-stages config <board>           # print all resolved variables
crossdev-stages config <board> --diff    # diff against defaults
```
Useful for debugging board.conf issues.

### `enter` -- enter build sandbox with board context
```
crossdev-stages enter <board>            # shell in sandbox with sysroot + build mounted
crossdev-stages enter <board> --step assemble  # shell at specific step's state
```
Like `docker exec` into a running/stopped container.

### `cache` -- binary package cache management
```
crossdev-stages cache list               # list cached packages per sysroot
crossdev-stages cache size               # disk usage
crossdev-stages cache clean [--sysroot <name>]  # remove cached packages
```

### `doctor` -- diagnose common issues
```
crossdev-stages doctor                   # check sandbox, crossdev, sysroots, deps
```
Checks: sandbox exists + prepared, crossdev installed, sysroots valid,
host deps present (pyelftools, pkg-resources, etc.).
