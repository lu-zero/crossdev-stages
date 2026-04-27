use camino::{Utf8Path, Utf8PathBuf};

use crate::board::BoardConfig;
use crate::container::{destroy_dir, unpack_tarball, OverlaySpec, SandboxRunner};
use crate::error::{Error, Result};
use crate::portage::{install_host_deps, MakeConf};
use crate::stage::gentoo_profile;
use crate::workspace::Workspace;

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
    /// Idempotent: skips if `.prepared` marker exists.
    pub fn prepare(&self, mirror: Option<&str>) -> Result<()> {
        if self.dir.join(".prepared").exists() {
            tracing::info!("Sandbox already prepared, skipping.");
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
        }
        .write(&self.dir.join("etc/portage"))?;

        tracing::info!("Installing host dependencies…");
        install_host_deps(&self.runner())?;

        std::fs::write(self.dir.join(".prepared"), "")?;
        tracing::info!("Sandbox prepared.");
        Ok(())
    }

    /// Set up the crossdev toolchain for `target_arch` with `board`'s
    /// CFLAGS.  Output lives in the workspace's content-addressed store
    /// at `store/<chost>/<cflags-hash>/`; subsequent runs that need this
    /// toolchain overlay-mount the store dir at `/usr/<chost>/`.
    ///
    /// Idempotent: skips if `<store>/.complete` already exists.  The
    /// per-sandbox marker `.crossdev-<target_arch>` records the hash so
    /// status drift detection has a per-sandbox view.
    pub fn setup_crossdev(
        &self,
        ws: &Workspace,
        target_arch: &str,
        board: &BoardConfig,
    ) -> Result<()> {
        let chost = format!("{target_arch}-unknown-linux-gnu");
        let cflags = board.effective_cflags();
        let (_canonical, hash) = crate::cflags::canonicalize(&cflags);

        let store_dir = ws.store_dir().join(&chost).join(&hash);
        let complete_marker = store_dir.join(".complete");
        let portage_db_dir = store_dir.join(".portage-db");
        let sandbox_marker = self.dir.join(format!(".crossdev-{target_arch}"));

        if complete_marker.exists() {
            // Stores produced before db isolation have .complete but no
            // .portage-db.  The empty bind-mount would trick subsequent
            // cross-emerges into thinking nothing is installed and
            // refusing to operate; force a rebuild instead.
            let needs_rebuild = !portage_db_dir.exists()
                || std::fs::read_dir(&portage_db_dir)
                    .map(|mut it| it.next().is_none())
                    .unwrap_or(true);
            if needs_rebuild {
                tracing::warn!(
                    "Store {store_dir} predates db isolation; rebuilding"
                );
                std::fs::remove_file(&complete_marker).ok();
            } else {
                tracing::info!(
                    "Crossdev prefix at {store_dir} already complete (cflags hash {hash}), skipping."
                );
                std::fs::write(&sandbox_marker, &hash)?;
                return Ok(());
            }
        }

        std::fs::create_dir_all(&store_dir)?;
        let portage_db_dir = store_dir.join(".portage-db");
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
            );

        // Clean up the repos.conf entry `eselect repository` wrote pointing
        // at a project overlay that may not exist yet. Portage nags about
        // the missing directory on every emerge otherwise. When project/
        // board overlays actually arrive in Phase 2, we'll regenerate this
        // entry with the correct target.
        runner.run(
            "rm -f /etc/portage/repos.conf/crossdev-stages.conf",
        )?;

        tracing::info!("Creating crossdev overlay…");
        runner.run(
            "eselect repository list -i | grep -q crossdev \
             || eselect repository create crossdev",
        )?;

        tracing::info!("Initialising crossdev for {chost}…");
        runner.run(&format!("crossdev {chost} --init-target"))?;

        // Allow unstable rust-std and gcc-16 prerelease.
        runner.run(&format!(
            "echo 'cross-{chost}/rust-std **' \
             > /etc/portage/package.accept_keywords/rust-std"
        ))?;
        runner.run(
            "echo '<sys-devel/gcc-16.0.9999:16 **' \
             > /etc/portage/package.accept_keywords/gcc",
        )?;

        tracing::info!("Emerging gcc:16 (host)…");
        runner.run("emerge -b -k sys-devel/gcc:16")?;

        // Query the installed gcc-16 version and make it the default.
        let gcc_ver =
            runner.run_output("qlist -ICev sys-devel/gcc:16 | head -n1 | sed 's|.*/gcc-||'")?;
        if gcc_ver.is_empty() {
            return Err(Error::CommandFailed {
                code: 1,
                reason: "Could not determine gcc-16 version".into(),
            });
        }
        let gcc_profile =
            runner.run_output("gcc-config -l | grep '16' | head -n1 | awk '{print $2}'")?;
        runner.run(&format!("gcc-config {gcc_profile}"))?;
        runner.run("source /etc/profile && env-update")?;

        // Configure the crossdev prefix portage settings.  The store dir is
        // bind-mounted at /usr/<chost>/, so writing to <store>/etc/portage
        // on the host is identical to writing to /usr/<chost>/etc/portage
        // inside the sandbox.
        let crossdev_portage = store_dir.join("etc/portage");
        runner.run(&format!(
            "export PORTAGE_CONFIGROOT=/usr/{chost}; eselect profile set {profile}"
        ))?;
        self.write_crossdev_portage(&crossdev_portage, target_arch, &chost, &cflags, board)?;

        // Fix the split-usr layout created by crossdev.
        runner.run(&format!("mkdir -p /usr/{chost}/bin"))?;
        runner.run(&format!("merge-usr --root /usr/{chost}"))?;

        tracing::info!("Running crossdev (this takes a while)…");
        runner.run(&format!(
            "crossdev {chost} \
             --gcc {gcc_ver} \
             --ex-pkg sys-devel/clang-crossdev-wrappers \
             --ex-pkg sys-devel/rust-std"
        ))?;

        // Switch cross compiler to gcc-16.
        runner.run(&format!("gcc-config {chost}-16 && source /etc/profile"))?;

        std::fs::write(&complete_marker, &hash)?;
        std::fs::write(&sandbox_marker, &hash)?;
        tracing::info!(
            "Crossdev prefix at {store_dir} complete (cflags hash {hash})."
        );
        Ok(())
    }

    /// Build a [`SandboxRunner`] that overlay-mounts the workspace store
    /// for `(target_arch, board.cflags)` at `/usr/<chost>/`.  Use this
    /// for any operation that reads or writes the cross-toolchain (image
    /// builds, target updates, etc.).  The lower (immutable store) must
    /// already be marked complete by [`Self::setup_crossdev`].
    pub fn runner_for_board(
        &self,
        ws: &Workspace,
        target_arch: &str,
        board: &BoardConfig,
    ) -> Result<SandboxRunner> {
        let (_canonical, hash) = crate::cflags::canonicalize(&board.effective_cflags());
        self.runner_for_chost(ws, target_arch, &hash)
    }

    /// Lower-level variant of [`Self::runner_for_board`] that takes the
    /// cflags-hash directly.  Useful for target operations that don't have
    /// a board, only an arch (e.g. `target build-stage1`).
    pub fn runner_for_chost(
        &self,
        ws: &Workspace,
        target_arch: &str,
        cflags_hash: &str,
    ) -> Result<SandboxRunner> {
        let chost = format!("{target_arch}-unknown-linux-gnu");
        let store_dir = ws.store_dir().join(&chost).join(cflags_hash);
        if !store_dir.join(".complete").exists() {
            return Err(Error::CommandFailed {
                code: 1,
                reason: format!(
                    "store {store_dir} is not complete; run setup_crossdev first"
                ),
            });
        }
        let portage_db_dir = store_dir.join(".portage-db");
        let binpkgs_cross_dir = store_dir.join(".binpkgs-cross");
        std::fs::create_dir_all(&portage_db_dir)?;
        std::fs::create_dir_all(&binpkgs_cross_dir)?;
        let upper_in_sandbox = format!(".overlay-upper-{chost}-{cflags_hash}");
        let work_in_sandbox = format!(".overlay-work-{chost}-{cflags_hash}");
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

    /// Write portage config files for the crossdev prefix directly on the host fs.
    fn write_crossdev_portage(
        &self,
        portage_dir: &Utf8Path,
        arch: &str,
        chost: &str,
        cflags: &str,
        board: &BoardConfig,
    ) -> Result<()> {
        // make.conf for the crossdev prefix
        MakeConf {
            arch,
            chost: Some(chost),
            cflags: Some(cflags),
            mirror: None,
            binhost: None,
            pkgdir: None,
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

        // package.accept_keywords
        std::fs::write(
            portage_dir.join("package.accept_keywords/gcc"),
            "<sys-devel/gcc-16.0.9999:16 **\n",
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
            let name = dir.file_name().unwrap_or("").to_string();
            SandboxInfo {
                name,
                arch,
                prepared,
            }
        })
        .collect())
}

pub struct SandboxInfo {
    pub name: String,
    pub arch: String,
    pub prepared: bool,
}
