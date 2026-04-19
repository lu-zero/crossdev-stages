# Design

## Overview

`crossdev-stages` builds bootable Gentoo images for embedded targets via
rootless cross-compilation.  It replaces a 1700-line bash script
(`sandbox-stage.sh`) with a typed Rust CLI that wraps
[hakoniwa](https://github.com/souk4711/hakoniwa) (Linux user-namespace
containers) and [crossdev](https://wiki.gentoo.org/wiki/Crossdev).

The key property is **rootless**: the entire build runs as an unprivileged
user.  No `sudo`, no root chroot — hakoniwa user-namespace containers provide
the isolation.

---

## Terminology

Terms are kept consistent with [catalyst](https://wiki.gentoo.org/wiki/Catalyst)
and the cross-compilation toolchain (GCC/crossdev) conventions.

| Term | Meaning |
|---|---|
| **source stage** | The stage3 tarball used as the seed for a new sandbox or target stage.  Catalyst: `source_path`. |
| **sandbox** | An unpacked amd64 stage3 that serves as the host build environment.  Analogous to catalyst's `chroot_path`, but rootless (hakoniwa). |
| **target stage** | The cross-compiled Gentoo root filesystem being built (mounted at `/target` inside the sandbox container during cross-emerge). |
| **crossdev prefix** | The `/usr/<chost>` tree inside the sandbox where crossdev installs the cross-toolchain (compiler, headers, stage1 libs).  Not a "sysroot" — that term is reserved for the `--sysroot` compiler flag. |
| **build** | A per-board working directory under `builds/` used during the image pipeline. |
| **board** | A hardware target described by `boards/<name>/board.conf` and optional hook scripts. |
| **stage1** | The bootstrap phase for a target stage: cross-emerge `baselayout` → `packages.build` → `portage`.  Mirrors catalyst's stage1 concept. |

---

## Workspace layout

Everything lives under `~/.cache/crossdev-stages/`:

```
~/.cache/crossdev-stages/
  stages/      Downloaded stage3 source tarballs (keyed by arch + variant).
  sandboxes/   Unpacked host build environments.
  targets/     Cross-compiled target stage roots.
  builds/      Per-board image build working directories.
  sources/     Bare-repo git source cache (kernel, u-boot, opensbi, …).
  logs/        Portage and build logs, bind-mounted from sandbox containers.
```

The project directory (where `boards/` lives) is separate and passed via
`--project-dir` (default: current directory).

---

## Key abstractions

### `Sandbox`

An amd64 Gentoo root unpacked from a source stage.  Provides:

- `create(ws, name, arch, source_stage)` — unpack a stage3, write `.arch` marker.
- `prepare(mirror)` — run `emerge-webrsync`, install host build dependencies
  (crossdev, rust, dracut, genimage, …), write `.prepared` marker.
- `setup_crossdev(arch, board)` — install the cross-toolchain for `arch` into
  the crossdev prefix (`/usr/<chost>`), write `.crossdev-<arch>` marker.
- `runner()` — return a `SandboxRunner` for executing commands inside the
  container.

All operations are idempotent via marker files.

### `Target`

The cross-compiled Gentoo root filesystem for the target arch.  Stored
under `targets/<name>/` and mounted read-write at `/target` inside the
sandbox container during cross-emerge.  Provides:

- `create(ws, name, arch, source_stage)` — unpack a stage3 as the starting point.
- `build_stage1(sandbox)` — bootstrap: cross-emerge `baselayout`,
  `packages.build`, `portage`.  Writes `.stage1` marker when complete.
- `update(sandbox)` — update the crossdev prefix toolchain then rebuild
  `@world` in the target stage.
- `install(sandbox, packages)` — cross-emerge specific packages into the
  target stage.

### `SandboxRunner`

Wraps `hakoniwa::Container` to run commands inside a sandbox.  The four
mount configurations mirror the original bash script's `run*` functions:

| Method | `/target` | `/build` | `/scripts` | `/cache` |
|---|---|---|---|---|
| `runner()` | — | — | — | — |
| `.with_target(dir)` | rw | — | — | — |
| `.with_build(dir, scripts)` | — | rw | ro | — |
| `.with_cache(dir)` | — | — | — | rw |

### `Build`

A per-board working directory for the image pipeline.  Resumable: if an
unpacked build for the same board exists without a `.packed` marker, it is
reused rather than recreated.

### `BoardConfig`

Parsed from `boards/<name>/board.conf` (shell key=value + bash array syntax).
Holds arch, CFLAGS, kernel/bootloader repo references, boot configuration,
and per-package CFLAGS workarounds.

---

## Image pipeline

Steps run in order (configurable via `BUILD_STEPS` in `board.conf`):

```
deps       Install extra sandbox packages + cross-emerge target-packages.txt.
checkout   Clone/update kernel, opensbi, u-boot source via git source cache.
bootloader Build opensbi and/or u-boot inside the sandbox container.
kernel     Build Linux kernel + modules; install modules into target stage.
assemble   Build dracut initramfs; layout /build/gen/root from target stage.
pack       Run genimage to produce the final disk image; compress.
```

Each step checks a `.{step}` marker for idempotency.  For each step,
`boards/<name>/` is checked for hook scripts:

```
override-{step}.sh   Replaces the Rust default entirely.
pre-{step}.sh        Runs before the Rust default.
post-{step}.sh       Runs after the Rust default.
```

Hook scripts run inside the sandbox container with `/scripts` bind-mounted
to the project directory (read-only) so they can source `board.conf` and
sibling helpers.

---

## Sandboxing approach

hakoniwa creates a user-namespace container (unshares Mount, User, Pid, Ipc,
Uts, Cgroup) but keeps the host network.  The caller's UID is mapped to
container root (uid 0) using `/etc/subuid` and `/etc/subgid`, which allows
portage to create files owned by system users (portage, nobody, …) inside
the sandbox without real root.

The sandbox root (`sandbox_dir`) is mounted read-write so portage can
install packages normally.  `/var/log` is bind-mounted from
`~/.cache/crossdev-stages/logs/<sandbox-name>/` so build logs survive
outside the container.

---

## Relationship to catalyst

catalyst builds Gentoo stages natively (same arch) using a real chroot.
`crossdev-stages` targets cross-compilation to embedded arches and uses
rootless containers instead.  The vocabulary intentionally mirrors catalyst
where the concepts overlap:

- **source stage** ↔ catalyst `source_path` (seed tarball)
- **target stage** ↔ catalyst's output stage (stage1/stage3 artifact)
- **stage1 bootstrap** ↔ catalyst `target: stage1`
- **sandbox** ↔ catalyst `chroot_path` (but rootless via hakoniwa)

The target stage produced by `target export` is a standard stage3-compatible
tarball that catalyst can use as a `source_path`.
