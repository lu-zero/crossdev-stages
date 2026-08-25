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
fedora   deps unpacks a pinned container base image, then installs
         fedora-packages.txt with dnf5 from the crossdev-stages overlay;
         assemble writes the same systemd config as debian.  Seeding
         needs no emulation; installing does.
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
fedora   split.  Unpacking the base image executes nothing, so a board
         with an empty fedora-packages.txt builds on a host with no
         binfmt at all.  Installing packages does need it: rpm runs each
         scriptlet inside the installroot and glibc's own file trigger
         execs ldconfig there.  Turning that off (tsflags=noscripts)
         also kills the file triggers, so it is not offered.
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
about to authenticate would buy nothing.

apk-tools itself is not in ::gentoo, so the crossdev-stages overlay
carries `app-arch/apk-tools` (category following ::gentoo's own
`app-arch/dpkg` and `app-arch/rpm`) and the deps step emerges it.  An
ebuild rather than a pinned binary: portage enforces the release
tarball's checksum from the Manifest, the sandbox VDB records what was
built and against which USE flags, and the result is cached as a binpkg
like every other host package instead of being a blob nothing accounts
for.

### The fedora provider, and what Fedora actually publishes

Seed and install are separate problems here, and Fedora answers them
very differently.

**Seed.**  Fedora composes one OCI container base image per
architecture per release, with a published checksum, so `deps` is a
download, a sha256 and two `tar` calls.  The archive is an OCI image
layout rather than a flat rootfs tarball: `index.json` plus
`blobs/sha256/*`, with the tree in the single layer blob.  URL, compose
id and sha256 for each (release, arch) are pinned in `provider.rs`;
a release that is not in that table is refused rather than guessed at.

**What the seed is not** is an operating system.  Fedora builds the
container base from a KIWI profile that ignores `kernel` and installs
`systemd-standalone-sysusers` instead of `systemd`, so the tree has
bash, coreutils, glibc, rpm and dnf5 (147 packages) and no PID 1.  The
profile that does carry systemd, `Container-Base-Generic-Init`, is not
published; the artifacts that boot are Cloud qcow2 and Server raw disk
images, neither of which is a tarball, and extracting one would mean a
1.3 GB download and ~10 GB of scratch to reach a root partition 2.6 GB
in.  So `deps` says plainly when the root has no init, and a board that
wants one lists `systemd` in fedora-packages.txt.

**Install.**  ::gentoo has no dnf.  Verified against the tree in the
sandbox, not the web index: no libsolv, no librepo, no libcomps, no
libdnf, no dnf, in any category.  It is three packages short, not a
dozen: `dev-libs/libsolv`, `dev-libs/librepo` and `sys-apps/dnf5`,
vendored into `defaults/overlay/` from ::guru at b23748630f89, which
carries and maintains all three.  Their Manifests were regenerated
locally and match ::guru's byte for byte, so the tarball hashes have two
independent sources.  dnf5 dropped libcomps, and everything else it
needs (app-arch/rpm, dev-cpp/sdbus-c++, sys-libs/libmodulemd,
app-arch/zchunk, dev-cpp/toml11, dev-libs/libfmt, json-c, glib) is
already in ::gentoo.  42 packages get pulled in the first time, once
per sandbox.

No repository configuration is written, because the image already
carries Fedora's own and it is per-arch correct: the riscv64 image
enables `[fedora-riscv]` and disables the primary metalink, the primary
images do the opposite.  `--nogpgcheck` is never passed; the release
key is in the image's rpmdb as a `gpg-pubkey`, so aarch64 and x86_64
signatures are checked by Fedora's own rules.

### riscv64 on Fedora, honestly

riscv64 is not a Fedora architecture.  It is not in
`releases/<rel>/Everything/`, which is aarch64 and x86_64, and not in
`fedora-secondary/`, which is ppc64le and s390x.  The RISC-V SIG
composes separately and publishes images under `/pub/alt/risc-v/`
(the provider knows the different path) and packages from its own koji
at `riscv-koji.fedoraproject.org`.  Release numbering keeps up (44
today, same as primary) and coverage is effectively complete.

Two things about it are worse than primary, and both are Fedora's, not
this code's:

- **Unsigned.**  Fedora's own `fedora-riscv.repo` ships `gpgcheck=0`.
  No `RPM-GPG-KEY-fedora-<rel>-riscv64` exists (`fedora-repos`'s
  archmap lists x86_64, aarch64, ppc64le and s390x for the primary
  key, and the per-arch names are symlinks to it).  Reading the
  signature header of a riscv64 RPM shows no RSA tag, no DSA tag and no
  PGP tag, against an aarch64 package of the same NVR that has them.
  The dist-repo's own `repo.json` records
  `allow_missing_signatures: true`.  `deps` says this out loud when it
  installs.
- **Unpinnable.**  The repo is
  `repos-dist/f<rel>/latest/<arch>`, and `latest` is a symlink koji
  moves; the numbered repos behind it are garbage-collected.  The base
  image is a fixed URL with a fixed checksum, so the *seed* is
  reproducible; anything installed on top of it is not.

The images have a matching gap: primary composes publish a clearsigned
`Fedora-Container-<rel>-<compose>-<arch>-CHECKSUM`, the SIG's publish a
bare `.sha256` sidecar.  Which is why the checksums are committed in
`provider.rs` rather than fetched: for riscv64 that value is the entire
trust anchor.

### The other two architectures

armv7 and i586 have no Fedora at all, and `fedora_arch()` refuses them
rather than 404ing partway through.  ARMv7 was retired in Fedora 37;
36 is the last release with an `armhfp` tree and it went EOL in 2023.
32-bit x86 last had a full tree in 30 (secondary) and 25 (primary).
What is still built i686 today is multilib libraries: `glibc`,
`systemd`, `util-linux` and `rpm-libs` have i686 builds, `bash`,
`coreutils`, `rpm` and `filesystem` do not, so there is no first
transaction to run, never mind i586's missing SSE2.

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
