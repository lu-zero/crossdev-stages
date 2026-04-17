# crossdev-stages

Build bootable Gentoo images for RISC-V and aarch64 SBCs using crossdev in
hakoniwa user-namespace sandboxes — no root, no chroot, no distro lock-in.

## Status

- [x] Cross-compiled stage1 + update cycle
- [x] OpenSBI / U-Boot / TF-A / rkbin bootloaders (modular)
- [x] Kernel + modules + DTB + initramfs
- [x] Bootable image assembly (genimage → ext4/FAT/MBR/GPT)
- [x] hakoniwa user-namespace sandboxing (no root)
- [x] Per-step hooks (`pre-/post-/override-<step>.sh`)
- [x] Git source cache (bare repo references)
- [x] Declarative config layout ([docs/configuration.md](docs/configuration.md))
- [x] Auto-applied kernel patches and config fragments per board

## Platforms

- **riscv64** — BPI-F3 (k1), Milk-V Jupiter, DC Roma II, OrangePi RV2,
  Canaan K230, Kickyard KY-X1, Tenstorrent Blackhole P100/P150
- **aarch64** — Odroid M2 (testing)

## Quick start

```sh
# One-shot: sets up sandbox + target + builds image
crossdev-stages image build --board k230

# Export the artifact
crossdev-stages image export k230 -o /tmp/
```

Under the hood, `image build` auto-creates the host sandbox (downloads stage3
if needed), prepares it (emerges host deps, runs crossdev), creates a target
stage1, then runs the 6-step pipeline (`deps → checkout → bootloader →
kernel → assemble → pack`).

For finer control, each step has its own subcommand:

```sh
crossdev-stages sandbox setup --arch x86_64      # download + unpack stage3
crossdev-stages sandbox prepare                  # emerge host deps + crossdev
crossdev-stages sandbox crossdev --arch riscv64  # set up cross-toolchain
crossdev-stages target stage1 --arch riscv64     # cross-emerge @system
crossdev-stages image build --board blackhole
crossdev-stages status                           # overview of all artefacts
```

## CLI

```
crossdev-stages [OPTIONS] <COMMAND>

Commands:
  sandbox  Manage host build sandboxes
  target   Manage cross-compiled target stages
  board    Manage and inspect boards
  image    Build board images
  stages   List or download Gentoo stage3 tarballs
  maint    Maintenance: cleanup, logs, diagnostics
  status   Show overview of sandboxes, targets, builds, and boards

Options:
  --project-dir <DIR>       Project root where boards/ lives [default: .]
  --mirror <URL>            Gentoo mirror URL
  --binhost <URL>           Binary package host URL (PORTAGE_BINHOST)
  --portage-overlay <DIR>   Extra /etc/portage fragments on top of defaults
  --dry-run                 Show what would be done
```

## Configuration

Everything is file-driven: embedded defaults in
`crossdev-stages/config/` plus optional per-board files under
`boards/<name>/`. Full reference: [docs/configuration.md](docs/configuration.md).

Adding a new board = drop a `boards/<name>/board.conf` + `genimage.cfg`,
optionally a kernel patch and config fragment. No Rust changes needed.

## Host requirements

- Linux with user namespaces enabled (`CONFIG_USER_NS=y`) and an unprivileged
  sub-UID range (see `/etc/subuid`)
- [hakoniwa](https://github.com/souk4711/hakoniwa) installed on the host
- Rust toolchain (for building crossdev-stages itself)

crossdev-stages emerges its own Gentoo dependencies (crossdev, dracut,
busybox, genimage, etc.) inside the sandbox, so the host doesn't need
Gentoo — any Linux distro with the above works.

## Roadmap

See [docs/cli-roadmap.md](docs/cli-roadmap.md) for the CLI-level roadmap and
this session's [plan file](.claude/plans) for the ongoing config/library
refactor.

Longer-term: overlayfs-based sandbox layering (share base stage3 +
host-deps across boards), standalone crate API (`crossdev-stages` as a
cross-rs replacement for Rust projects), and Rust 2024 edition migration.
