use std::collections::HashMap;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use portage_atom::Version as PortageVersion;

use crate::board::BoardConfig;
use crate::container::{
    destroy_dir, recover_mounts_for_removal, unpack_tarball, OverlaySpec, SandboxRunner,
};
use crate::error::{Error, Result};
use crate::portage::{install_host_deps, sync_portage_tree, MakeConf};
use crate::stage::gentoo_profile;
use crate::workspace::{store_key, Workspace};

/// A Gentoo sandbox: an unpacked stage3 used as the host build environment.
pub struct Sandbox {
    pub dir: Utf8PathBuf,
    pub arch: String,
}

impl Sandbox {
    /// Open an existing sandbox directory, reading its `.arch` marker.
    pub fn open(dir: Utf8PathBuf) -> Result<Self> {
        let arch = std::fs::read_to_string(dir.join(".arch"))
            .map(|s| s.trim().to_string())
            .map_err(|_| Error::SandboxNotFound(dir.to_string()))?;
        Ok(Self { dir, arch })
    }

    /// Create a new sandbox by unpacking a stage3 source tarball (catalyst: `source_path`).
    /// Writes a `.arch` marker on success.
    pub fn create(ws: &Workspace, name: &str, arch: &str, source_stage: &Utf8Path) -> Result<Self> {
        let dir = ws.sandbox(name);
        if dir.is_dir() {
            tracing::info!("Sandbox {} already exists, skipping unpack.", name);
            return Self::open(dir);
        }
        tracing::info!("Unpacking stage3 into {}…", dir);
        unpack_tarball(source_stage, &dir, ws.base())?;
        std::fs::write(dir.join(".arch"), arch)?;
        tracing::info!("Sandbox {} created.", name);
        Ok(Self {
            dir,
            arch: arch.to_string(),
        })
    }

    /// Configure portage and install host build dependencies.
    /// Idempotent: skips if `.prepared` marker exists (or `.prepared-bare` when `bare`).
    ///
    /// With `bare`, writes `make.conf` and syncs the portage tree but does not
    /// emerge packages.  When `<defaults_root>/overlay/` exists, installs it
    /// as the `crossdev-stages` portage overlay (for opt-in extras like the
    /// ESOS firmware ebuilds).
    pub fn prepare(&self, mirror: Option<&str>, defaults_root: &Utf8Path, bare: bool) -> Result<()> {
        // The overlay refreshes on every prepare, even on an already-prepared
        // sandbox: defaults/overlay/ is the source of truth and the copy is
        // cheap and idempotent.
        install_overlay(&self.runner(), &self.dir, defaults_root)?;

        if self.dir.join(".prepared").exists() {
            tracing::info!("Sandbox already prepared, skipping.");
            return Ok(());
        }
        if bare && self.dir.join(".prepared-bare").exists() {
            tracing::info!("Sandbox already bare-prepared, skipping.");
            return Ok(());
        }
        tracing::info!("Configuring portage…");
        MakeConf {
            arch: &self.arch,
            chost: None,
            cflags: None,
            mirror,
            binhost: None,
            pkgdir: None,
            for_build_host: true,
        }
        .write(&self.dir.join("etc/portage"))?;

        // No gcc pin in the host sandbox — stage3's default gcc runs portage itself.
        crate::portage::write_version_pins(&self.dir.join("etc/portage"), None)?;

        if bare {
            sync_portage_tree(&self.runner())?;
            std::fs::write(self.dir.join(".prepared-bare"), "")?;
            tracing::info!("Sandbox bare-prepared.");
        } else {
            tracing::info!("Installing host dependencies…");
            install_host_deps(&self.runner(), defaults_root, &self.dir.join("etc/portage"))?;
            std::fs::write(self.dir.join(".prepared"), "")?;
            let _ = std::fs::remove_file(self.dir.join(".prepared-bare"));
            tracing::info!("Sandbox prepared.");
        }
        Ok(())
    }

    /// Return installed GCC versions grouped by slot, using `.gcc_versions` cache if present.
    /// Versions within each slot are sorted newest-first.
    pub fn get_installed_gcc_versions(&self) -> Result<HashMap<String, Vec<String>>> {
        let cache = self.dir.join(".gcc_versions");
        if let Ok(content) = fs::read_to_string(&cache) {
            if let Ok(parsed) = toml::from_str::<HashMap<String, Vec<String>>>(&content) {
                return Ok(parsed);
            }
        }
        self.refresh_gcc_versions()
    }

    /// Query installed GCC versions fresh via `qlist` and rewrite the `.gcc_versions` cache.
    fn refresh_gcc_versions(&self) -> Result<HashMap<String, Vec<String>>> {
        let output = self.runner().run_output("qlist -ICev sys-devel/gcc")?;
        let mut versions: HashMap<String, Vec<String>> = HashMap::new();
        for line in output.lines() {
            let line = line.trim();
            if let Some(ver_str) = line.split("/gcc-").last().filter(|s| !s.is_empty()) {
                if let Ok(ver) = PortageVersion::parse(ver_str) {
                    let slot = ver.numbers[0].to_string();
                    versions.entry(slot).or_default().push(ver_str.to_string());
                }
            }
        }
        for vs in versions.values_mut() {
            vs.sort_by(|a, b| b.cmp(a)); // newest first
        }
        let _ = fs::write(
            self.dir.join(".gcc_versions"),
            toml::to_string(&versions).unwrap_or_default(),
        );
        Ok(versions)
    }

    /// The gcc spec used to key the store when no explicit version is
    /// requested: the highest installed slot.  Deterministic and known
    /// host-side (`.gcc_versions` cache; one `qlist` run the first time)
    /// before any crossdev work enters the sandbox.
    pub fn default_gcc_spec(&self) -> Result<String> {
        let installed = self.get_installed_gcc_versions()?;
        installed
            .keys()
            .filter_map(|k| k.parse::<u32>().ok())
            .max()
            .map(|n| n.to_string())
            .ok_or_else(|| Error::CommandFailed {
                code: 1,
                reason: "No GCC installed in sandbox; run sandbox prepare first".into(),
            })
    }

    /// Resolve the gcc spec that keys the store for `board`:
    /// CLI `--gcc-version` > `BOARD_GCC_VERSION` > highest installed slot.
    /// The spec string is used verbatim in the store key, so the same
    /// resolution must be applied by everything that touches the store
    /// (setup, runners, portage prep).
    pub fn gcc_spec_for(&self, board: &BoardConfig, cli_gcc: Option<&str>) -> Result<String> {
        match cli_gcc.or(board.gcc_version.as_deref()) {
            Some(s) => Ok(s.to_string()),
            None => self.default_gcc_spec(),
        }
    }

    /// Set up the crossdev toolchain for `target_arch` with `board`'s CFLAGS.
    ///
    /// The cross-prefix output lives in the workspace's content-addressed
    /// store at `store/<chost>/<cflags-hash>-gcc<spec>/`; subsequent runs
    /// that need this toolchain overlay-mount the store dir at
    /// `/usr/<chost>/` (see [`Self::runner_for_board`]).  The gcc spec is
    /// part of the key: two boards with the same chost+CFLAGS but different
    /// `BOARD_GCC_VERSION` get separate prefixes instead of ping-ponging
    /// full rebuilds inside one dir.
    ///
    /// GCC version resolution order: CLI `gcc_version`, then
    /// `board.gcc_version`, then highest installed slot.  The spec is either
    /// a bare slot number ("15") or a version prefix ("15.2",
    /// "15.2.1_p20260214").  Prefixes use portage's `=pkg-ver*` glob, so
    /// "15.2" matches any 15.2.x snapshot.
    ///
    /// Idempotent: a store dir carrying `.complete` is never rebuilt (a
    /// different gcc spec or CFLAGS keys a different dir), and never
    /// mutated while `.complete` is set -- the marker is written only
    /// after the full setup finishes.
    pub fn setup_crossdev(
        &self,
        ws: &Workspace,
        target_arch: &str,
        board: &BoardConfig,
        gcc_version: Option<&str>,
    ) -> Result<()> {
        let chost = crate::stage::chost_for_arch(target_arch)?;
        let cflags = board.effective_cflags();
        let (_canonical, hash) = crate::cflags::canonicalize(&cflags);

        // Resolve gcc spec from CLI > board > auto-detect.
        // A single number ("15") selects the slot; any longer version ("15.2",
        // "15.2.1_p…") is used as a portage version prefix via
        // =sys-devel/gcc-<prefix>*.
        let requested = gcc_version.or(board.gcc_version.as_deref());
        let (gcc_slot, ver_prefix): (String, Option<String>) = match requested {
            None => (self.default_gcc_spec()?, None),
            Some(s) => {
                let ver = PortageVersion::parse(s).map_err(|_| Error::CommandFailed {
                    code: 1,
                    reason: format!("BOARD_GCC_VERSION {s:?} is not a valid portage version"),
                })?;
                let slot = ver.numbers[0].to_string();
                // A single bare number ("15") means slot-only; anything longer is a prefix.
                let is_slot_only =
                    ver.numbers.len() == 1 && ver.letter.is_none() && ver.suffixes.is_empty();
                if is_slot_only {
                    (slot, None)
                } else {
                    (slot, Some(s.to_string()))
                }
            }
        };
        // The spec that keys the store: the requested string verbatim, or
        // the auto-detected slot.  Known host-side before any sandbox work.
        let gcc_spec = requested
            .map(str::to_string)
            .unwrap_or_else(|| gcc_slot.clone());

        let store_dir = ws.store_dir().join(store_key(&chost, &hash, &gcc_spec));
        let complete_marker = store_dir.join(".complete");
        let portage_db_dir = store_dir.join(".portage-db");
        let sandbox_marker = self.dir.join(format!(".crossdev-{target_arch}"));
        // Shared target binpkg cache: the crossdev prefix make.conf sets
        // PKGDIR=/binpkgs, so every runner that executes {chost}-emerge
        // must bind this dir there.
        let binpkgs_dir = ws.binpkgs_dir().join(&chost).join(&hash);
        std::fs::create_dir_all(&binpkgs_dir)?;

        // Idempotency: `.complete` in a (chost, cflags-hash, gcc-spec) keyed
        // dir means this exact toolchain is built.  Only per-sandbox glue is
        // (re-)ensured here; the store itself is not touched.  Exception:
        // a bare `.complete` without a populated `.portage-db` predates db
        // isolation -- treat it as stale, clear the marker, rebuild.
        if complete_marker.exists() {
            let db_populated = portage_db_dir.exists()
                && std::fs::read_dir(&portage_db_dir)
                    .map(|mut it| it.next().is_some())
                    .unwrap_or(false);
            if !db_populated {
                tracing::warn!("Store {store_dir} predates db isolation; rebuilding");
                std::fs::remove_file(&complete_marker).ok();
            } else {
                let existing = std::fs::read_to_string(&complete_marker)
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                tracing::info!(
                    "Crossdev prefix at {store_dir} complete (gcc-{existing}), skipping."
                );
                // A sandbox that never ran this setup (fresh unpack, or one
                // last used with a different gcc) lacks the host-side
                // compiler driver for this store; replay it before anything
                // runs {chost}-emerge.  Maintains the sandbox marker.
                self.ensure_host_payload(target_arch, &chost, &store_dir)?;
                // Ensure any board-required ex-pkgs are present even on the skip path.
                self.ensure_grub_ex_pkg(board, &chost, &store_dir, &binpkgs_dir)?;
                return Ok(());
            }
        }

        // From here the dir is partial (no `.complete`): building into it
        // is safe, and a crash leaves it partial, never half-complete.
        std::fs::create_dir_all(&store_dir)?;
        let binpkgs_cross_dir = store_dir.join(".binpkgs-cross");
        std::fs::create_dir_all(&portage_db_dir)?;
        std::fs::create_dir_all(&binpkgs_cross_dir)?;
        tracing::info!("Building crossdev prefix into {store_dir}…");

        let profile = gentoo_profile(target_arch)?;
        // Bind-mount the store dir RW at /usr/<chost>/ so the crossdev
        // wizard's writes land directly in the workspace store.  Also
        // bind the per-(chost, hash) portage db dir at
        // /var/db/pkg/cross-<chost>/ and the matching cross-toolchain
        // binpkg cache at /var/cache/binpkgs/cross-<chost>/, so portage's
        // installed-packages record AND the cached cross-gcc/glibc
        // tarballs stay in sync with the store contents.  Without this,
        // the host sandbox's shared db/binpkgs let one (chost, hash)
        // install shadow another's, and the wizard either fast-paths to
        // a no-op or merges binaries built for a different cflags-hash.
        let runner = self
            .runner()
            .with_extra_rw(&store_dir, &format!("/usr/{chost}"))
            .with_extra_rw(&portage_db_dir, &format!("/var/db/pkg/cross-{chost}"))
            .with_extra_rw(
                &binpkgs_cross_dir,
                &format!("/var/cache/binpkgs/cross-{chost}"),
            )
            .with_binpkgs(&binpkgs_dir);

        tracing::info!("Creating crossdev overlay…");
        runner.run(
            "eselect repository list -i | grep -q crossdev \
             || eselect repository create crossdev",
        )?;

        tracing::info!("Initialising crossdev for {chost}…");
        runner.run(&format!("crossdev {chost} --init-target"))?;

        // Accept testing/prerelease keywords for this gcc slot and rust-std.
        runner.run(&format!(
            "echo 'cross-{chost}/rust-std **' \
             > /etc/portage/package.accept_keywords/rust-std"
        ))?;
        let gcc_keyword_line = format!("sys-devel/gcc:{gcc_slot} **");
        runner.run(&format!(
            "echo '{gcc_keyword_line}' > /etc/portage/package.accept_keywords/gcc"
        ))?;

        // Emerge: version prefix glob (quoted to prevent shell expansion) or best-in-slot.
        if let Some(ref prefix) = ver_prefix {
            tracing::info!("Emerging =sys-devel/gcc-{prefix}* (host)…");
            runner.run(&format!("emerge -b -k '=sys-devel/gcc-{prefix}*'"))?;
        } else {
            tracing::info!("Emerging sys-devel/gcc:{gcc_slot} (host)…");
            runner.run(&format!("emerge -b -k sys-devel/gcc:{gcc_slot}"))?;
        }

        // Refresh metadata after emerge — never use the pre-emerge cache here.
        let installed = self.refresh_gcc_versions()?;
        let slot_versions = installed.get(&gcc_slot).cloned().unwrap_or_default();

        let gcc_ver = if let Some(ref prefix) = ver_prefix {
            slot_versions
                .into_iter()
                .find(|v| v.starts_with(prefix.as_str()))
                .ok_or_else(|| Error::CommandFailed {
                    code: 1,
                    reason: format!("No gcc-{prefix}* found in sandbox after emerge"),
                })?
        } else {
            // Slot-only: newest installed version (refresh gives newest-first).
            slot_versions
                .into_iter()
                .next()
                .ok_or_else(|| Error::CommandFailed {
                    code: 1,
                    reason: format!("No gcc:{gcc_slot} found in sandbox after emerge"),
                })?
        };

        tracing::info!("Using gcc-{gcc_ver} for crossdev.");

        // gcc-config profile names are "{chost}-{slot}" (e.g. "aarch64-unknown-linux-gnu-15"),
        // not "{chost}-{full-version}". Select by slot directly.
        let host_chost = runner
            .run_output("portageq envvar CHOST")?
            .trim()
            .to_string();
        runner.run(&format!("gcc-config {host_chost}-{gcc_slot}"))?;
        runner.run("env-update && source /etc/profile")?;

        // Configure the crossdev prefix portage settings.  The store dir is
        // bind-mounted at /usr/<chost>/, so writing to <store>/etc/portage
        // on the host is identical to writing to /usr/<chost>/etc/portage
        // inside the sandbox.
        let crossdev_portage = store_dir.join("etc/portage");
        runner.run(&format!(
            "export PORTAGE_CONFIGROOT=/usr/{chost}; eselect profile set {profile}"
        ))?;
        self.write_crossdev_portage(
            &crossdev_portage,
            target_arch,
            &chost,
            &cflags,
            board,
            &gcc_keyword_line,
        )?;

        // Pin gcc + llvm in the cross prefix so future emerges (e.g.
        // `target update`'s cross_emerge_crossdev sys-devel/gcc) don't
        // silently jump to a different major.
        let gcc_pin = ver_prefix.as_deref().or(board.gcc_version.as_deref());
        crate::portage::write_version_pins(&crossdev_portage, gcc_pin)?;

        // Fix the split-usr layout created by crossdev.
        runner.run(&format!("mkdir -p /usr/{chost}/bin"))?;
        runner.run(&format!("merge-usr --root /usr/{chost}"))?;

        // The prefix gets a passwd/group from acct-user/portage alone, so it
        // knows "portage" but not "root".  Any ebuild reaching fowners with a
        // root:<group> pair then dies at install time.
        runner.run(&format!(
            "grep -q '^root:' /usr/{chost}/etc/passwd 2>/dev/null || \
             echo 'root:x:0:0:root:/root:/bin/bash' >> /usr/{chost}/etc/passwd; \
             grep -q '^root:' /usr/{chost}/etc/group 2>/dev/null || \
             echo 'root:x:0:' >> /usr/{chost}/etc/group"
        ))?;

        tracing::info!("Running crossdev (this takes a while)…");
        let grub_ex_pkg = if board.grub_platforms.is_some() {
            " --ex-pkg sys-boot/grub"
        } else {
            ""
        };
        runner.run(&format!(
            "crossdev {chost} \
             --gcc {gcc_ver} \
             --ex-pkg sys-devel/clang-crossdev-wrappers \
             --ex-pkg sys-devel/rust-std{grub_ex_pkg}"
        ))?;

        // Switch cross compiler to the installed slot.
        runner.run(&format!(
            "gcc-config {chost}-{gcc_slot} && source /etc/profile"
        ))?;

        // Stash the host-side toolchain payload in the store so a fresh
        // sandbox can replay it (see ensure_host_payload).  Must happen
        // before `.complete` -- a store without the payload is not usable
        // from any sandbox but this one.
        self.capture_host_payload(&runner, &chost)?;

        // Two markers, written only now that the prefix is fully built:
        // store `.complete` (exact gcc PVR; runner_for_chost requires it)
        // and per-sandbox `.crossdev-<arch>` (gcc of the toolchain whose
        // host payload this sandbox currently carries).
        std::fs::write(&complete_marker, &gcc_ver)?;
        std::fs::write(&sandbox_marker, &gcc_ver)?;
        tracing::info!("Crossdev prefix at {store_dir} complete (gcc-{gcc_ver}).");
        Ok(())
    }

    /// Build a [`SandboxRunner`] that overlay-mounts the workspace store
    /// for `(target_arch, board.cflags, board gcc spec)` at `/usr/<chost>/`.
    /// Use this for any operation that reads or writes the cross-toolchain
    /// (image builds, target updates, etc.).  The lower (immutable store)
    /// must already be marked complete by [`Self::setup_crossdev`].
    pub fn runner_for_board(
        &self,
        ws: &Workspace,
        target_arch: &str,
        board: &BoardConfig,
    ) -> Result<SandboxRunner> {
        let hash = crate::cflags::toolchain_key(board);
        let gcc_spec = self.gcc_spec_for(board, None)?;
        self.runner_for_chost(ws, target_arch, &hash, &gcc_spec)
    }

    /// Lower-level variant of [`Self::runner_for_board`] that takes the
    /// cflags-hash and gcc spec directly.  Useful for target operations
    /// that don't have a board, only an arch (e.g. `target stage1`).
    pub fn runner_for_chost(
        &self,
        ws: &Workspace,
        target_arch: &str,
        cflags_hash: &str,
        gcc_spec: &str,
    ) -> Result<SandboxRunner> {
        let chost = crate::stage::chost_for_arch(target_arch)?;
        let store_dir = ws
            .store_dir()
            .join(store_key(&chost, cflags_hash, gcc_spec));
        if !store_dir.join(".complete").exists() {
            return Err(Error::CommandFailed {
                code: 1,
                reason: format!("store {store_dir} is not complete; run setup_crossdev first"),
            });
        }
        // The store only captures /usr/<chost>/; the compiler driver and
        // friends live on the sandbox rootfs.  Replay them if this sandbox
        // doesn't carry them (fresh unpack, or last used with another gcc).
        self.ensure_host_payload(target_arch, &chost, &store_dir)?;
        let portage_db_dir = store_dir.join(".portage-db");
        let binpkgs_cross_dir = store_dir.join(".binpkgs-cross");
        std::fs::create_dir_all(&portage_db_dir)?;
        std::fs::create_dir_all(&binpkgs_cross_dir)?;
        // Upper/work are keyed by the full store key so two stores that
        // share chost+cflags but differ in gcc never mix upper layers.
        let leaf = store_dir.file_name().unwrap_or(cflags_hash);
        let upper_in_sandbox = format!(".overlay-upper-{chost}-{leaf}");
        let work_in_sandbox = format!(".overlay-work-{chost}-{leaf}");
        std::fs::create_dir_all(self.dir.join(&upper_in_sandbox))?;
        std::fs::create_dir_all(self.dir.join(&work_in_sandbox))?;
        Ok(self
            .runner()
            .with_overlay(OverlaySpec {
                lower: store_dir,
                upper_in_container: format!("/{upper_in_sandbox}"),
                work_in_container: format!("/{work_in_sandbox}"),
                mount_at: format!("/usr/{chost}"),
            })
            .with_extra_rw(&portage_db_dir, &format!("/var/db/pkg/cross-{chost}"))
            .with_extra_rw(
                &binpkgs_cross_dir,
                &format!("/var/cache/binpkgs/cross-{chost}"),
            ))
    }

    /// Return a `SandboxRunner` for running commands inside this sandbox.
    /// Logs are bind-mounted from `~/.cache/crossdev-stages/logs/<name>/`
    /// so they are accessible outside the sandbox at a known flat path.
    pub fn runner(&self) -> SandboxRunner {
        let name = self.dir.file_name().unwrap_or_default();
        let log_dir = self
            .dir
            .parent() // sandboxes/
            .and_then(|p| p.parent()) // <workspace>/
            .map(|ws| ws.join("logs").join(name))
            .unwrap_or_else(|| self.dir.join("var/log"));
        SandboxRunner::new(&self.dir, log_dir)
    }

    #[allow(dead_code)]
    pub fn is_prepared(&self) -> bool {
        self.dir.join(".prepared").exists()
    }

    #[allow(dead_code)]
    pub fn has_crossdev(&self, target_arch: &str) -> bool {
        self.dir.join(format!(".crossdev-{target_arch}")).exists()
    }

    // ── Private helpers ──────────────────────────────────────────────────────

    /// Tar everything crossdev installed OUTSIDE `/usr/<chost>/` into
    /// `<store>/.host-payload.tar`.
    ///
    /// The store bind captures only the prefix; the compiler itself lands
    /// on the setup sandbox's rootfs: the gcc driver + cc1
    /// (`/usr/<host-chost>/<chost>/gcc-bin/`, `/usr/lib/gcc/<chost>/`,
    /// `/usr/libexec/gcc/<chost>/`), the gcc-config/binutils-config wrapper
    /// symlinks and crossdev's emerge wrappers in `/usr/bin/<chost>-*`,
    /// `/etc/env.d/{gcc,binutils}/` entries, and the crossdev overlay repo.
    /// Enumerated from the cross-<chost>/* VDB CONTENTS (exact package
    /// payload) plus globs for the config-tool-generated state that no
    /// package owns.  `runner` must have the store bound at /usr/<chost>/.
    fn capture_host_payload(&self, runner: &SandboxRunner, chost: &str) -> Result<()> {
        tracing::info!("Capturing host-side toolchain payload for {chost} into the store…");
        runner.run(&format!(
            "{{ sed -n -e 's|^obj \\(/.*\\) [0-9a-f]* [0-9]*$|\\1|p' \
                    -e 's|^sym \\(/.*\\) -> .*|\\1|p' \
                    -e 's|^dir \\(/.*\\)$|\\1|p' \
                 /var/db/pkg/cross-{chost}/*/CONTENTS; \
               find /usr/bin -maxdepth 1 -name '{chost}-*'; \
               find /etc/env.d -name '*{chost}*'; \
               find /var/db/repos/crossdev /etc/portage/repos.conf; \
             }} 2>/dev/null | grep -v '^/usr/{chost}/' | sort -u \
             | tar -cpf /usr/{chost}/.host-payload.tar \
                   --no-recursion --ignore-failed-read -T -"
        ))
    }

    /// Replay a store's `.host-payload.tar` onto this sandbox's rootfs when
    /// the sandbox doesn't already carry that exact toolchain (checked via
    /// the `/usr/bin/<chost>-gcc` wrapper and the `.crossdev-<arch>` marker
    /// against the gcc PVR recorded in the store's `.complete`).  Idempotent;
    /// alternating between stores of the same chost re-points the wrapper
    /// symlinks each time.
    fn ensure_host_payload(
        &self,
        target_arch: &str,
        chost: &str,
        store_dir: &Utf8Path,
    ) -> Result<()> {
        let recorded = std::fs::read_to_string(store_dir.join(".complete"))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let marker = self.dir.join(format!(".crossdev-{target_arch}"));
        let marker_matches = std::fs::read_to_string(&marker)
            .map(|s| s.trim() == recorded)
            .unwrap_or(false);
        // symlink_metadata, not exists(): the wrapper is an absolute
        // symlink that only resolves inside the sandbox root.
        let driver_present =
            std::fs::symlink_metadata(self.dir.join(format!("usr/bin/{chost}-gcc"))).is_ok();
        if driver_present && marker_matches {
            return Ok(());
        }
        let payload = store_dir.join(".host-payload.tar");
        if !payload.is_file() {
            return Err(Error::CommandFailed {
                code: 1,
                reason: format!(
                    "sandbox lacks the {chost} toolchain and store {store_dir} has no \
                     .host-payload.tar; delete the store dir and re-run setup"
                ),
            });
        }
        tracing::info!("Replaying host-side toolchain payload for {chost} (gcc-{recorded})…");
        let runner = self.runner().with_extra_ro(store_dir, "/.store-payload");
        runner.run("tar -xpf /.store-payload/.host-payload.tar -C / && env-update")?;
        std::fs::write(&marker, &recorded)?;
        Ok(())
    }

    /// Install `sys-boot/grub` into the store-resident crossdev prefix if the
    /// board needs it and it isn't already there.  Uses `{chost}-emerge`
    /// (installs into `/usr/{chost}/`), which compiles grub with the
    /// i586/x86 cross-compiler, producing proper i386-pc modules regardless
    /// of the build host's native architecture.
    ///
    /// This amends a completed store, so `.complete` is dropped for the
    /// duration of the emerge and restored afterwards -- a crash mid-emerge
    /// leaves the store partial (rebuilt next run) instead of complete but
    /// permanently grubless.
    fn ensure_grub_ex_pkg(
        &self,
        board: &BoardConfig,
        chost: &str,
        store_dir: &Utf8Path,
        binpkgs_dir: &Utf8Path,
    ) -> Result<()> {
        let Some(ref platforms) = board.grub_platforms else {
            return Ok(());
        };
        if store_dir.join("usr/lib/grub/i386-pc").exists() {
            return Ok(());
        }
        let complete_marker = store_dir.join(".complete");
        let recorded = std::fs::read_to_string(&complete_marker).ok();
        if recorded.is_some() {
            std::fs::remove_file(&complete_marker)?;
        }
        // Write USE flags to the crossdev prefix portage config before emerging.
        // The crossdev portage config is not re-written when setup_crossdev is
        // skipped, so we must ensure the grub platform flag is present here.
        let flags: Vec<String> = platforms
            .split_whitespace()
            .map(|p| format!("grub_platforms_{p}"))
            .collect();
        let use_dir = store_dir.join("etc/portage/package.use");
        std::fs::create_dir_all(&use_dir)?;
        std::fs::write(
            use_dir.join("grub"),
            format!("sys-boot/grub {}\n", flags.join(" ")),
        )?;
        tracing::info!("Installing sys-boot/grub into crossdev prefix for {chost}…");
        // RW-bind the store (not an overlay) so grub persists in the store
        // itself and is visible to every sandbox that mounts it.
        let runner = self
            .runner()
            .with_extra_rw(store_dir, &format!("/usr/{chost}"))
            .with_binpkgs(binpkgs_dir);
        runner.run(&format!("{chost}-emerge -b -k sys-boot/grub"))?;
        if let Some(v) = recorded {
            std::fs::write(&complete_marker, v)?;
        }
        Ok(())
    }

    /// Write portage config files for the crossdev prefix directly on the host fs.
    fn write_crossdev_portage(
        &self,
        portage_dir: &Utf8Path,
        arch: &str,
        chost: &str,
        cflags: &str,
        board: &BoardConfig,
        gcc_keyword_line: &str,
    ) -> Result<()> {
        // make.conf for the crossdev prefix.  {chost}-emerge runs with
        // PORTAGE_CONFIGROOT=/usr/<chost> and reads THIS make.conf, not
        // the target's -- FEATURES=buildpkg + PKGDIR must live here or
        // they never take effect.  /binpkgs is the in-sandbox bind of
        // the shared binpkgs/<chost>/<cflags-hash>/ cache.
        MakeConf {
            arch,
            chost: Some(chost),
            cflags: Some(cflags),
            mirror: None,
            binhost: None,
            pkgdir: Some("/binpkgs"),
            for_build_host: true,
        }
        .write(portage_dir)?;

        for sub in [
            "env",
            "package.env",
            "package.use",
            "package.accept_keywords",
        ] {
            std::fs::create_dir_all(portage_dir.join(sub))?;
        }

        // env/plain.conf: strip arch-specific flags (used for rust, etc.)
        std::fs::write(
            portage_dir.join("env/plain.conf"),
            "CFLAGS=\"-O3 -pipe\"\nCXXFLAGS=\"-O3 -pipe\"\n",
        )?;

        // package.env
        std::fs::write(
            portage_dir.join("package.env/rust"),
            "dev-lang/rust plain.conf\n",
        )?;

        // package.use
        std::fs::write(
            portage_dir.join("package.use/busybox"),
            ">=virtual/libcrypt-2-r1 static-libs\n\
             >=sys-libs/libxcrypt-4.4.36-r3 static-libs\n\
             >=sys-apps/busybox-1.36.1-r3 -pam static\n",
        )?;
        std::fs::write(
            portage_dir.join("package.use/clang"),
            "llvm-core/clang -extra\n",
        )?;
        std::fs::write(
            portage_dir.join("package.use/rust"),
            "dev-lang/rust rustfmt -system-llvm\n",
        )?;
        std::fs::write(portage_dir.join("package.use/git"), "dev-vcs/git -iconv\n")?;

        if let Some(ref platforms) = board.grub_platforms {
            let flags: Vec<String> = platforms
                .split_whitespace()
                .map(|p| format!("grub_platforms_{p}"))
                .collect();
            std::fs::write(
                portage_dir.join("package.use/grub"),
                format!("sys-boot/grub {}\n", flags.join(" ")),
            )?;
        }

        // package.accept_keywords
        std::fs::write(
            portage_dir.join("package.accept_keywords/gcc"),
            format!("{gcc_keyword_line}\n"),
        )?;

        // Per-package CFLAGS workarounds from board.conf
        for (pkg, flags) in board
            .workaround_pkgs
            .iter()
            .zip(board.workaround_cflags.iter())
        {
            let safe_name = pkg.replace('/', "_");
            std::fs::write(
                portage_dir.join(format!("env/{safe_name}.conf")),
                format!("CFLAGS=\"{flags}\"\nCXXFLAGS=\"{flags}\"\n"),
            )?;
            std::fs::write(
                portage_dir.join(format!("package.env/{safe_name}")),
                format!("{pkg} {safe_name}.conf\n"),
            )?;
        }

        Ok(())
    }
}

/// Remove a sandbox directory (via hakoniwa to handle root-owned files from stage3).
pub fn destroy(ws: &Workspace, name: &str) -> Result<()> {
    let dir = ws.sandbox(name);
    if !dir.is_dir() {
        return Err(crate::error::Error::SandboxNotFound(name.into()));
    }
    println!("Removing sandbox: {name}");
    recover_mounts_for_removal(&dir, false)?;
    destroy_dir(&dir, ws.base())?;
    println!("Sandbox '{name}' removed.");
    Ok(())
}

/// List all sandbox directories with their state.
pub fn list(ws: &Workspace) -> Result<Vec<SandboxInfo>> {
    let dirs = ws.list_sandboxes()?;
    Ok(dirs
        .into_iter()
        .map(|dir| {
            let arch = crate::workspace::read_arch(&dir).unwrap_or_else(|| "unknown".into());
            let prepared = dir.join(".prepared").exists();
            let bare_prepared = dir.join(".prepared-bare").exists();
            let name = dir.file_name().unwrap_or("").to_string();
            SandboxInfo {
                name,
                arch,
                prepared,
                bare_prepared,
            }
        })
        .collect())
}

pub struct SandboxInfo {
    pub name: String,
    pub arch: String,
    pub prepared: bool,
    pub bare_prepared: bool,
}

/// Copy `<defaults_root>/overlay/` into the sandbox as the
/// `crossdev-stages` portage overlay and write a repos.conf entry.
/// No-op if the source overlay directory doesn't exist.
fn install_overlay(
    runner: &SandboxRunner,
    sandbox: &Utf8Path,
    defaults_root: &Utf8Path,
) -> Result<()> {
    let src = defaults_root.join("overlay");
    if !src.is_dir() {
        return Ok(());
    }
    let dst = sandbox.join("var/db/repos/crossdev-stages");
    tracing::info!("Installing crossdev-stages overlay at {dst}…");
    // /var/db/repos is created by portage inside the container, so it ends up
    // owned by a subordinate uid the host user cannot write to.  Container
    // root maps back to the caller, so let it create the directory; the copy
    // itself then lands in something the host owns.
    if !dst.is_dir() {
        runner.run("mkdir -p /var/db/repos/crossdev-stages")?;
    }
    copy_tree(&src, &dst)?;

    let repos_conf = sandbox.join("etc/portage/repos.conf");
    fs::create_dir_all(&repos_conf)?;
    fs::write(
        repos_conf.join("crossdev-stages.conf"),
        "[crossdev-stages]\n\
         location = /var/db/repos/crossdev-stages\n\
         auto-sync = no\n",
    )?;
    Ok(())
}

/// Recursively copy directory `src` into `dst`, overwriting files.
/// Symlinks and other special entries are rejected.
fn copy_tree(src: &Utf8Path, dst: &Utf8Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let from = Utf8PathBuf::try_from(entry.path()).map_err(|e| Error::CommandFailed {
            code: 1,
            reason: e.to_string(),
        })?;
        let to = dst.join(from.file_name().unwrap_or_default());
        if kind.is_dir() {
            copy_tree(&from, &to)?;
        } else if kind.is_file() {
            std::fs::copy(&from, &to)?;
        } else {
            return Err(Error::CommandFailed {
                code: 1,
                reason: format!("unsupported entry in portage overlay (symlink?): {from}"),
            });
        }
    }
    Ok(())
}
