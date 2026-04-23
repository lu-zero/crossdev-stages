use camino::{Utf8Path, Utf8PathBuf};

use crate::container::{destroy_dir, unpack_tarball};
use crate::error::{Error, Result};
use crate::portage::{MakeConf, Portage};
use crate::sandbox::Sandbox;
use crate::stage::default_cflags;
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
    /// Writes a `.arch` marker and a `.stage3` marker (file name only, for manifest).
    pub fn create(ws: &Workspace, name: &str, arch: &str, source_stage: &Utf8Path) -> Result<Self> {
        let dir = ws.target(name);
        if dir.is_dir() {
            tracing::info!("Target {} already exists, skipping unpack.", name);
            return Self::open(dir);
        }
        tracing::info!("Unpacking stage3 into target {}…", dir);
        unpack_tarball(source_stage, &dir, ws.base())?;
        std::fs::write(dir.join(".arch"), arch)?;
        if let Some(fname) = source_stage.file_name() {
            std::fs::write(dir.join(".stage3"), fname)?;
        }
        tracing::info!("Target {} created.", name);
        Ok(Self {
            dir,
            arch: arch.to_string(),
        })
    }

    /// Bootstrap the target: cross-emerge baselayout → packages.build → portage.
    /// Idempotent via `.stage1` marker.
    pub fn build_stage1(&self, sandbox: &Sandbox) -> Result<()> {
        if self.dir.join(".stage1").exists() {
            tracing::info!("Stage1 already built, skipping.");
            return Ok(());
        }
        let chost = format!("{}-unknown-linux-gnu", self.arch);

        // Write target portage config and copy profile before first emerge.
        tracing::info!("Preparing target portage configuration…");
        self.prepare_portage(sandbox, &chost)?;

        let runner = sandbox.runner().with_target(&self.dir);
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

        self.update_ldconfig(sandbox)?;

        std::fs::write(self.dir.join(".stage1"), chrono::Utc::now().to_rfc3339())?;
        tracing::info!("Stage1 complete.");
        Ok(())
    }

    /// Update the target stage (`@world` rebuild).
    pub fn update(&self, sandbox: &Sandbox) -> Result<()> {
        let chost = format!("{}-unknown-linux-gnu", self.arch);
        let runner = sandbox.runner().with_target(&self.dir);
        let portage = Portage::new(&runner);

        // Update the cross-toolchain in the crossdev prefix first (no ROOT=/target).
        tracing::info!("Updating crossdev prefix: gcc, binutils-libs, @system…");
        portage.cross_emerge_crossdev(&chost, &["sys-devel/gcc"])?;
        portage.cross_emerge_crossdev(&chost, &["sys-libs/binutils-libs"])?;
        portage.cross_emerge_crossdev(&chost, &["-u", "system"])?;

        // Rebuild @world in the target.
        tracing::info!("Rebuilding @world in target…");
        runner.run(&format!(
            "KERNEL_DIR=/usr/src/linux ROOT=/target {chost}-emerge -b -k -e @world"
        ))?;

        self.update_ldconfig(sandbox)?;
        std::fs::write(self.dir.join(".updated"), chrono::Utc::now().to_rfc3339())?;
        Ok(())
    }

    /// Cross-emerge specific packages into the target.
    pub fn install(&self, sandbox: &Sandbox, packages: &[&str]) -> Result<()> {
        let chost = format!("{}-unknown-linux-gnu", self.arch);
        let runner = sandbox.runner().with_target(&self.dir);
        let portage = Portage::new(&runner);
        portage.cross_emerge(&chost, packages)
    }

    /// Run `ldconfig` inside the target stage.
    pub fn update_ldconfig(&self, sandbox: &Sandbox) -> Result<()> {
        tracing::info!("Updating ldconfig in target…");
        let runner = sandbox.runner().with_target(&self.dir);
        runner.run("ldconfig -v -r /target")
    }

    /// Write target portage make.conf and copy the profile link from the
    /// crossdev prefix in the sandbox — mirrors `prepare_target_portage` in
    /// the bash script.
    fn prepare_portage(&self, sandbox: &Sandbox, chost: &str) -> Result<()> {
        let portage_dir = self.dir.join("etc/portage");
        std::fs::create_dir_all(&portage_dir)?;

        MakeConf {
            arch: &self.arch,
            chost: Some(chost),
            cflags: Some(default_cflags(&self.arch)),
            mirror: None,
            binhost: None,
        }
        .write(&portage_dir)?;

        // Copy the profile directory and make.profile symlink from the
        // crossdev prefix so the target stage uses the correct Gentoo profile.
        let src_portage = sandbox.dir.join(format!("usr/{chost}/etc/portage"));

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
