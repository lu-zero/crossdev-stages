# Design

## Overview

`crossdev-stages` cross-compiles Gentoo stages for foreign architectures,
running entirely as an unprivileged user.  It replaces a 1700-line bash
script (`sandbox-stage.sh`) with a typed Rust CLI that wraps
[hakoniwa](https://github.com/souk4711/hakoniwa) (Linux user-namespace
containers) and [crossdev](https://wiki.gentoo.org/wiki/Crossdev).

The primary outputs are:

- **A target stage** — a cross-compiled Gentoo root that can be exported as
  a standard stage3-compatible tarball for use with
  [catalyst](https://wiki.gentoo.org/wiki/Catalyst) or native bootstrapping.
- **A bootable image** — kernel, bootloader, and disk image built on top of
  the target stage for a specific board.

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
| **build** | A working directory under `builds/<board>/<timestamp>/` used during the image pipeline; one fresh leaf per build. |
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
  builds/      Image build working directories, nested builds/<board>/<timestamp>/.
  sources/     Bare-repo git source cache (kernel, u-boot, opensbi, …).
  logs/        Portage and build logs, bind-mounted from sandbox containers.
  store/       Content-addressed crossdev prefix store (chost + CFLAGS hash + gcc).
  binpkgs/     Shared binary-package cache (PKGDIR), keyed by chost + CFLAGS hash.
```

The project directory (where `boards/` lives) is separate and passed via
`--project-dir` (default: current directory).

---

## Key abstractions

### `Sandbox`

A Gentoo root for the host arch unpacked from a source stage, used as the
build environment for cross-compilation.  Provides:

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

### `RootfsProvider`

Who fills and configures the image's root filesystem, selected by
`ROOTFS_PROVIDER` in board.conf (absent → `gentoo`).  The provider owns
three seams — how `/target` is seeded, what the `deps` step installs,
and the os-config tail of `assemble` — while checkout, kernel,
bootloader, assemble mechanics, and pack stay provider-agnostic.

```
gentoo   stage3 seed; deps cross-emerges defaults + board lists;
         assemble writes OpenRC config.  The default; also the only
         provider that unconditionally needs the crossdev toolchain.
debian   deps runs debootstrap in the sandbox; board extras install via
         --include from debian-packages.txt; assemble writes systemd
         config (fstab, hostname, hosts, serial-getty@, root password).
ubuntu   the same, with ubuntu-packages.txt, app-crypt/ubuntu-keyring,
         the ports.ubuntu.com mirror off amd64/i386, and no rolling
         suite alias to default to.
alpine   deps runs a static apk in the sandbox; board extras come from
         alpine-packages.txt; assemble writes OpenRC config.  Needs no
         emulation at all (see below).
none     nothing seeded, installed, or configured; board hook scripts
         (override-deps.sh, post-assemble.sh, ...) own the rootfs.
```

Providers other than gentoo set up the toolchain store only when
BUILD_STEPS compiles target code (kernel/bootloader).

### `SecondStage`

`debootstrap --foreign` downloads every .deb but unpacks only the
Priority:required set, and neither Debian nor Ubuntu puts an init in
that set (systemd-sysv is Priority:important).  Who finishes the job is
`ROOTFS_SECOND_STAGE`:

```
chroot      the default: `chroot /target /debootstrap/debootstrap
            --second-stage` at deps time.  Executes target binaries, so
            a foreign arch needs qemu-user binfmt with the F flag on the
            host.  What every other image builder does, and the reason
            the shipped image is a finished system.
first-boot  opt-in for a host that cannot register binfmt.  deps stops
            after stage 1 and assemble installs
            defaults/scripts/debootstrap-second-stage.init at /sbin/init
            -- free, because stage 1 leaves no init there.  The shim
            runs the second stage from the .debs in the image, blanks
            root's password (its /etc/shadow does not exist until then),
            runs apt-get clean, then replaces itself with the real init
            and execs it.  Costs a roughly doubled image and a slow
            first boot; a failure is recorded rather than hidden.
```

The `.debootstrap-done` marker in the target records which mode built
it, so flipping the key re-bootstraps instead of shipping a rootfs the
board.conf no longer describes.

### Emulation, per provider

The seam splits three ways, not two, and the split is what decides
whether a board can be built on a host with no binfmt registration:

```
gentoo   no emulation.  Everything is cross-compiled; nothing target-arch
         is ever executed.
alpine   no emulation.  apk is a host-arch binary that only reads and
         writes files, and --no-scripts stops it exec'ing the packages'
         shell scriptlets; upstream documents that flag for exactly this
         ("useful for extracting a system image for different
         architecture on alternative ROOT").
debian   qemu-user binfmt required.  debootstrap's second stage runs
ubuntu   target binaries in a chroot, and there is no equivalent of
         --no-scripts, because that stage *is* the configuration.
         ROOTFS_SECOND_STAGE="first-boot" moves that stage onto the
         board rather than removing it; see `SecondStage` above.
```

`--no-scripts` is not free.  Three packages in the alpine-base closure
carry install scriptlets, and the deps step reproduces their effects
without running them: busybox's applet symlinks, which are the whole
userland including /sbin/init; alpine-baselayout's shadow group; and
openrc's, which is a no-op on a fresh root.  Any *other* package whose
scriptlet did not run is named in the build log, read back from apk's own
record of the scripts it stored, so a board can handle it from
post-assemble.sh.

Signatures are checked for every non-Gentoo provider.  Debian and Ubuntu
each pass their own `--keyring`.  Alpine's per-architecture signing keys
are committed under `defaults/alpine-keys/<arch>/` and copied into the
root before the first `apk add`, so `--allow-untrusted` is never needed:
they are the trust anchor, and fetching an anchor over the channel it is
about to authenticate would buy nothing.  apk-tools itself is not in
::gentoo, so it is fetched from a pinned URL and checked against a pinned
sha256 (both in `provider.rs`).

### Why there is no fedora provider

`dnf --installroot=/target --forcearch=<arch> --releasever=<n> install
@core` is the right shape and the flag is long since merged, but two
things stop it here, and the first is decisive:

- ::gentoo carries no dnf.  Checked across every category of the tree:
  no `dnf`, no `dnf5`, no `yum`, no `libdnf`, no `libsolv`.  What is
  there is `app-arch/rpm` and `app-arch/createrepo_c`, which give you an
  unpacker and an indexer, not a dependency resolver.  The sandbox is a
  Gentoo stage3, so there is nothing to emerge and nothing to run.
- Even with dnf, rpm scriptlets execute during the transaction, and
  `--forcearch`'s own documented prerequisite is qemu-user-static plus a
  binfmt registration.  Fedora would land in the debian bucket above,
  not the alpine one, so it would not be the emulation-free provider
  that motivated adding a third.

A provider that needs a package manager the sandbox cannot install is a
stub, so the enum does not carry the variant.  Reviving it needs an
in-tree dnf, or a vendored static one pinned the way apk-tools is, and it
would still be documented as needing binfmt.

---

## Image pipeline

Steps run in order (configurable via `BUILD_STEPS` in `board.conf`):

```
deps       Cross-emerge defaults + per-board package lists into the target.
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

## Package lists

`defaults/` holds package lists applied everywhere:

- `defaults/sandbox-packages.txt` — host build deps emerged during
  `sandbox prepare` (required).
- `defaults/target-packages.txt` — packages cross-emerged into every
  image's sysroot during `deps` (required).

Per-board lists (`boards/<name>/sandbox-packages.txt`, `target-packages.txt`)
overlay the defaults.  The effective target set is
`defaults/target-packages.txt` UNION `boards/<name>/target-packages.txt`
MINUS the board's `-atom` lines (e.g. `-app-misc/fastfetch`) — every part
is a plain file.  Subtracting an atom not in the merged set warns and does
nothing; `-atom` lines in a defaults file are rejected with an error, since
defaults define the base and only boards subtract.  Heavy per-board opt-ins
(mold, go, cmake, rust, iw, wpa_supplicant) are deliberately not defaults.
Sandbox defaults are installed at prepare time, so a `-atom` in a board's
`sandbox-packages.txt` can only cancel the board's own extras.

List lines are `atom [keywords]`: an optional keyword override (e.g.
`sys-boot/syslinux **`) is written to `etc/portage/package.accept_keywords/`
before emerging.  A board's `sandbox-packages.use` (`atom use_flags...`
lines) is written to `etc/portage/package.use/` the same way.

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
`crossdev-stages` cross-compiles to foreign arches and uses rootless
containers instead.  The vocabulary intentionally mirrors catalyst
where the concepts overlap:

- **source stage** ↔ catalyst `source_path` (seed tarball)
- **target stage** ↔ catalyst's output stage (stage1/stage3 artifact)
- **stage1 bootstrap** ↔ catalyst `target: stage1`
- **sandbox** ↔ catalyst `chroot_path` (but rootless via hakoniwa)

The target stage produced by `target export` is a standard stage3-compatible
tarball that catalyst can use as a `source_path`.
