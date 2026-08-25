use camino::{Utf8Path, Utf8PathBuf};

use crate::container::{destroy_dir, unpack_tarball};
use crate::error::{Error, Result};
use crate::portage::{MakeConf, Portage};
use crate::sandbox::Sandbox;
use crate::stage::{chost_for_arch, default_cflags};
use crate::workspace::Workspace;

/// A cross-compiled Gentoo stage for the target arch.
pub struct Target {
    pub dir: Utf8PathBuf,
    pub arch: String,
}

impl Target {
    pub fn open(dir: Utf8PathBuf) -> Result<Self> {
        let arch = std::fs::read_to_string(dir.join(".arch"))
            .map(|s| s.trim().to_string())
            .map_err(|_| Error::TargetNotFound(dir.to_string()))?;
        Ok(Self { dir, arch })
    }

    /// Create a new target stage by unpacking a stage3 source tarball (catalyst: `source_path`).
    /// Writes `.arch`, `.stage3` (file name only, for manifest) and `.provider` markers.
    pub fn create(ws: &Workspace, name: &str, arch: &str, source_stage: &Utf8Path) -> Result<Self> {
        let dir = ws.target(name);
        if dir.is_dir() {
            guard_provider(&dir, name, "gentoo")?;
            tracing::info!("Target {} already exists, skipping unpack.", name);
            return Self::open(dir);
        }
        tracing::info!("Unpacking stage3 into target {}…", dir);
        unpack_tarball(source_stage, &dir, ws.base())?;
        std::fs::write(dir.join(".arch"), arch)?;
        if let Some(fname) = source_stage.file_name() {
            std::fs::write(dir.join(".stage3"), fname)?;
        }
        std::fs::write(dir.join(".provider"), "gentoo")?;
        tracing::info!("Target {} created.", name);
        Ok(Self {
            dir,
            arch: arch.to_string(),
        })
    }

    /// Create an empty target: no stage3 seed, only the markers.  For
    /// rootfs providers that fill /target themselves; `.stage3` is set
    /// to "none" so the build lock records the absence instead of
    /// falling back to a cached tarball name, and `.provider` keeps the
    /// dir from ever being resolved for another provider's board.
    pub fn create_empty(ws: &Workspace, name: &str, arch: &str, provider: &str) -> Result<Self> {
        let dir = ws.target(name);
        if dir.is_dir() {
            guard_provider(&dir, name, provider)?;
            tracing::info!("Target {} already exists, skipping.", name);
            return Self::open(dir);
        }
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(".arch"), arch)?;
        std::fs::write(dir.join(".stage3"), "none")?;
        std::fs::write(dir.join(".provider"), provider)?;
        tracing::info!("Target {} created (empty).", name);
        Ok(Self {
            dir,
            arch: arch.to_string(),
        })
    }

    /// Bootstrap the target: cross-emerge baselayout → packages.build → portage.
    /// Idempotent via `.stage1` marker.
    pub fn build_stage1(&self, ws: &Workspace, sandbox: &Sandbox) -> Result<()> {
        if self.dir.join(".stage1").exists() {
            tracing::info!("Stage1 already built, skipping.");
            return Ok(());
        }
        let chost = chost_for_arch(&self.arch)?;
        // Standalone `target stage1` has no board context, so key the store
        // with generic arch defaults; image builds override later via
        // `prepare_portage_with_cflags(..., board.effective_cflags(), ...)`.
        let cflags = default_cflags(&self.arch);
        let (_, hash) = crate::cflags::canonicalize(cflags);
        let gcc_spec = sandbox.default_gcc_spec()?;
        let binpkgs_dir = ws.binpkgs_dir().join(&chost).join(&hash);
        std::fs::create_dir_all(&binpkgs_dir)?;

        tracing::info!("Preparing target portage configuration…");
        self.prepare_portage_with_cflags(ws, &chost, cflags, &gcc_spec)?;

        let runner = sandbox
            .runner_for_chost(ws, &self.arch, &hash, &gcc_spec)?
            .with_target(&self.dir)
            .with_binpkgs(&binpkgs_dir);
        tracing::info!("Logs at: {}", runner.log_dir());
        let portage = Portage::new(&runner);

        tracing::info!("Cross-emerging baselayout…");
        portage.cross_emerge_build(&chost, &["sys-apps/baselayout"])?;

        tracing::info!("Cross-emerging packages.build…");
        let packages = runner.run_output(
            "grep -v '^#' /var/db/repos/gentoo/profiles/default/linux/packages.build \
             | grep -v '^[[:space:]]*$' | tr '\\n' ' '",
        )?;
        if packages.is_empty() {
            return Err(crate::error::Error::CommandFailed {
                code: 1,
                reason: "packages.build is empty or missing".into(),
            });
        }
        runner.run(&format!("ROOT=/target {chost}-emerge -b -k {packages}"))?;

        tracing::info!("Cross-emerging portage…");
        portage.cross_emerge_build(&chost, &["sys-apps/portage"])?;

        self.update_ldconfig(ws, sandbox)?;

        std::fs::write(self.dir.join(".stage1"), chrono::Utc::now().to_rfc3339())?;
        tracing::info!("Stage1 complete.");
        Ok(())
    }

    /// Update the target stage (`@world` rebuild).
    pub fn update(&self, ws: &Workspace, sandbox: &Sandbox) -> Result<()> {
        let chost = chost_for_arch(&self.arch)?;
        let (_, hash) = crate::cflags::canonicalize(default_cflags(&self.arch));
        let gcc_spec = sandbox.default_gcc_spec()?;
        let binpkgs_dir = ws.binpkgs_dir().join(&chost).join(&hash);
        std::fs::create_dir_all(&binpkgs_dir)?;
        let runner = sandbox
            .runner_for_chost(ws, &self.arch, &hash, &gcc_spec)?
            .with_target(&self.dir)
            .with_binpkgs(&binpkgs_dir);
        let portage = Portage::new(&runner);

        // Update the cross-toolchain in the crossdev prefix first (no ROOT=/target).
        // Pin gcc to the spec the store key already names, so portage does not
        // pick whatever is currently default (a different major would break the
        // ABI of binpkgs already in the cache).  Pass --noreplace so the cross
        // prefix's
        // package.mask/pin-gcc — which intentionally blocks upgrades past the
        // installed version to prevent bootstrap breakage — does not abort the
        // run when gcc is already at the requested version.
        // Single-quoted: the atom goes through `bash -c` and an unquoted
        // `=sys-devel/gcc-15*` is subject to shell glob expansion
        // (sandbox.rs quotes the identical atom in setup_crossdev).
        let gcc_atom = format!("'=sys-devel/gcc-{gcc_spec}*'");
        tracing::info!(gcc_atom = %gcc_atom, "Updating crossdev prefix: gcc, binutils-libs, @system…");
        portage.cross_emerge_crossdev(&chost, &["--noreplace", &gcc_atom])?;
        portage.cross_emerge_crossdev(&chost, &["sys-libs/binutils-libs"])?;
        portage.cross_emerge_crossdev(&chost, &["-u", "system"])?;

        // Rebuild @world in the target.
        // Explicit --jobs / --load-average so EMERGE_DEFAULT_OPTS from make.conf
        // can't be silently lost (e.g. when a wrapper drops PORTAGE_CONFIGROOT).
        let (jobs, load) = crate::portage::parallelism();
        tracing::info!("Rebuilding @world in target…");
        runner.run(&format!(
            "KERNEL_DIR=/usr/src/linux ROOT=/target {chost}-emerge \
             -b -k --jobs={jobs} --load-average {load} -e @world"
        ))?;

        self.update_ldconfig(ws, sandbox)?;
        std::fs::write(self.dir.join(".updated"), chrono::Utc::now().to_rfc3339())?;
        Ok(())
    }

    /// Cross-emerge specific packages into the target.
    pub fn install(&self, ws: &Workspace, sandbox: &Sandbox, packages: &[&str]) -> Result<()> {
        let chost = chost_for_arch(&self.arch)?;
        let (_, hash) = crate::cflags::canonicalize(default_cflags(&self.arch));
        let gcc_spec = sandbox.default_gcc_spec()?;
        let binpkgs_dir = ws.binpkgs_dir().join(&chost).join(&hash);
        std::fs::create_dir_all(&binpkgs_dir)?;
        let runner = sandbox
            .runner_for_chost(ws, &self.arch, &hash, &gcc_spec)?
            .with_target(&self.dir)
            .with_binpkgs(&binpkgs_dir);
        let portage = Portage::new(&runner);
        portage.cross_emerge(&chost, packages)
    }

    /// Run `ldconfig` inside the target stage.
    pub fn update_ldconfig(&self, ws: &Workspace, sandbox: &Sandbox) -> Result<()> {
        tracing::info!("Updating ldconfig in target…");
        let (_, hash) = crate::cflags::canonicalize(default_cflags(&self.arch));
        let gcc_spec = sandbox.default_gcc_spec()?;
        let runner = sandbox
            .runner_for_chost(ws, &self.arch, &hash, &gcc_spec)?
            .with_target(&self.dir);
        runner.run("ldconfig -v -r /target")
    }

    /// Write target portage make.conf with explicit CFLAGS and copy the
    /// profile link from the crossdev prefix in the workspace store —
    /// mirrors `prepare_target_portage` in the bash script.  Idempotent:
    /// re-writes make.conf each call so callers can refresh CFLAGS when
    /// a board's values change.
    pub fn prepare_portage_with_cflags(
        &self,
        ws: &Workspace,
        chost: &str,
        cflags: &str,
        gcc_spec: &str,
    ) -> Result<()> {
        let portage_dir = self.dir.join("etc/portage");
        std::fs::create_dir_all(&portage_dir)?;

        // pkgdir is deliberately None: this make.conf ships into the image
        // via `cp -a /target/. /build/gen/root/`.  FEATURES=buildpkg +
        // PKGDIR=/binpkgs belong in the crossdev prefix config, which is
        // what {chost}-emerge (PORTAGE_CONFIGROOT=/usr/<chost>) reads.
        MakeConf {
            arch: &self.arch,
            chost: Some(chost),
            cflags: Some(cflags),
            mirror: None,
            binhost: None,
            pkgdir: None,
            for_build_host: false,
        }
        .write(&portage_dir)?;

        // The store key already resolved which gcc this target is built by.
        crate::portage::write_version_pins(&portage_dir, Some(gcc_spec))?;

        // Copy the profile directory and make.profile symlink from the
        // store-resident crossdev prefix so the target stage uses the
        // correct Gentoo profile.  The (chost, cflags-hash, gcc-spec)
        // keyed store dir is the source of truth post-Phase 3.
        let (_, hash) = crate::cflags::canonicalize(cflags);
        let src_portage = ws
            .store_dir()
            .join(crate::workspace::store_key(chost, &hash, gcc_spec))
            .join("etc/portage");

        let src_profile_dir = src_portage.join("profile");
        if src_profile_dir.is_dir() {
            let dst = portage_dir.join("profile");
            let status = std::process::Command::new("cp")
                .args(["-a", src_profile_dir.as_str(), dst.as_str()])
                .status()?;
            if !status.success() {
                return Err(Error::CommandFailed {
                    code: status.code().unwrap_or(-1),
                    reason: format!("cp -a {src_profile_dir} failed"),
                });
            }
        }

        let src_link = src_portage.join("make.profile");
        if src_link.is_symlink() {
            let link_target = std::fs::read_link(&src_link)?;
            let dst_link = portage_dir.join("make.profile");
            if dst_link.exists() || dst_link.is_symlink() {
                std::fs::remove_file(&dst_link)?;
            }
            std::os::unix::fs::symlink(&link_target, &dst_link)?;
        }

        Ok(())
    }
}

/// Refuse to adopt an existing target dir that another rootfs provider
/// created: silently reusing it hands e.g. an empty provider-owned tree
/// to the Gentoo pipeline (garbage image) or a stage3 tree to a
/// provider that would provision over it.
fn guard_provider(dir: &camino::Utf8Path, name: &str, provider: &str) -> Result<()> {
    let have = crate::workspace::read_provider(dir);
    if have != provider {
        return Err(Error::CommandFailed {
            code: 1,
            reason: format!(
                "target '{name}' belongs to rootfs provider '{have}', not '{provider}' \
                 -- pass --target or `target destroy {name}` first"
            ),
        });
    }
    Ok(())
}

/// Remove a target directory (via hakoniwa to handle root-owned files).
pub fn destroy(ws: &Workspace, name: &str) -> Result<()> {
    let dir = ws.target(name);
    if !dir.is_dir() {
        return Err(Error::TargetNotFound(name.into()));
    }
    println!("Removing target: {name}");
    destroy_dir(&dir, ws.base())?;
    println!("Target '{name}' removed.");
    Ok(())
}

/// List all target directories with their state.
pub fn list(ws: &Workspace) -> Result<Vec<TargetInfo>> {
    let dirs = ws.list_targets()?;
    Ok(dirs
        .into_iter()
        .map(|dir| {
            let arch = crate::workspace::read_arch(&dir).unwrap_or_else(|| "unknown".into());
            let stage1 = dir.join(".stage1").exists();
            let updated = std::fs::read_to_string(dir.join(".updated"))
                .ok()
                .map(|s| s.trim().to_string());
            let name = dir.file_name().unwrap_or("").to_string();
            TargetInfo {
                name,
                arch,
                stage1,
                updated,
            }
        })
        .collect())
}

pub struct TargetInfo {
    pub name: String,
    pub arch: String,
    pub stage1: bool,
    pub updated: Option<String>,
}
