use std::path::{Path, PathBuf};

use crate::board::BoardConfig;
use crate::container::{unpack_tarball, SandboxRunner};
use crate::error::{Error, Result};
use crate::portage::{install_host_deps, MakeConf};
use crate::stage::gentoo_profile;
use crate::workspace::Workspace;

/// A Gentoo sandbox: an unpacked stage3 used as the host build environment.
pub struct Sandbox {
    pub dir: PathBuf,
    pub arch: String,
}

impl Sandbox {
    /// Open an existing sandbox directory, reading its `.arch` marker.
    pub fn open(dir: PathBuf) -> Result<Self> {
        let arch = std::fs::read_to_string(dir.join(".arch"))
            .map(|s| s.trim().to_string())
            .map_err(|_| Error::SandboxNotFound(dir.display().to_string()))?;
        Ok(Self { dir, arch })
    }

    /// Create a new sandbox by unpacking a stage3 tarball.
    /// Writes a `.arch` marker on success.
    pub fn create(ws: &Workspace, name: &str, arch: &str, stage_file: &Path) -> Result<Self> {
        let dir = ws.sandbox(name);
        if dir.is_dir() {
            log::info!("Sandbox {} already exists, skipping unpack.", name);
            return Self::open(dir);
        }
        log::info!("Unpacking stage3 into {}…", dir.display());
        unpack_tarball(stage_file, &dir, ws.base())?;
        std::fs::write(dir.join(".arch"), arch)?;
        log::info!("Sandbox {} created.", name);
        Ok(Self { dir, arch: arch.to_string() })
    }

    /// Configure portage and install host build dependencies.
    /// Idempotent: skips if `.prepared` marker exists.
    pub fn prepare(&self, mirror: Option<&str>) -> Result<()> {
        if self.dir.join(".prepared").exists() {
            log::info!("Sandbox already prepared, skipping.");
            return Ok(());
        }
        log::info!("Configuring portage…");
        MakeConf {
            arch: &self.arch,
            chost: None,
            cflags: None,
            mirror,
        }
        .write(&self.dir.join("etc/portage"))?;

        log::info!("Installing host dependencies…");
        install_host_deps(&self.runner())?;

        std::fs::write(self.dir.join(".prepared"), "")?;
        log::info!("Sandbox prepared.");
        Ok(())
    }

    /// Set up the crossdev toolchain for `target_arch` inside this sandbox.
    /// Idempotent: skips if `.crossdev-<target_arch>` marker exists.
    pub fn setup_crossdev(&self, target_arch: &str, board: &BoardConfig) -> Result<()> {
        let marker = self.dir.join(format!(".crossdev-{target_arch}"));
        if marker.exists() {
            log::info!("Crossdev for {target_arch} already set up, skipping.");
            return Ok(());
        }

        let chost = format!("{target_arch}-unknown-linux-gnu");
        let profile = gentoo_profile(target_arch)?;
        let cflags = board.effective_cflags();
        let runner = self.runner();

        log::info!("Creating crossdev overlay…");
        runner.run(
            "eselect repository list -i | grep -q crossdev \
             || eselect repository create crossdev",
        )?;

        log::info!("Initialising crossdev for {chost}…");
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

        log::info!("Emerging gcc:16 (host)…");
        runner.run("emerge -b -k sys-devel/gcc:16")?;

        // Query the installed gcc-16 version and make it the default.
        let gcc_ver = runner.run_output(
            "qlist -ICev sys-devel/gcc:16 | head -n1 | sed 's|.*/gcc-||'",
        )?;
        if gcc_ver.is_empty() {
            return Err(Error::CommandFailed {
                code: 1,
                reason: "Could not determine gcc-16 version".into(),
            });
        }
        let gcc_profile = runner.run_output(
            "gcc-config -l | grep '16' | head -n1 | awk '{print $2}'",
        )?;
        runner.run(&format!("gcc-config {gcc_profile}"))?;
        runner.run("source /etc/profile && env-update")?;

        // Configure the cross-sysroot portage settings (written on the host fs).
        let crossdev_root = self.dir.join(format!("usr/{chost}"));
        let crossdev_portage = crossdev_root.join("etc/portage");
        runner.run(&format!(
            "export PORTAGE_CONFIGROOT=/usr/{chost}; eselect profile set {profile}"
        ))?;
        self.write_crossdev_portage(&crossdev_portage, target_arch, &chost, &cflags, board)?;

        // Fix the split-usr layout created by crossdev.
        runner.run(&format!("mkdir -p /usr/{chost}/bin"))?;
        runner.run(&format!("merge-usr --root /usr/{chost}"))?;

        log::info!("Running crossdev (this takes a while)…");
        runner.run(&format!(
            "crossdev {chost} \
             --gcc {gcc_ver} \
             --ex-pkg sys-devel/clang-crossdev-wrappers \
             --ex-pkg sys-devel/rust-std"
        ))?;

        // Switch cross compiler to gcc-16.
        runner.run(&format!("gcc-config {chost}-16 && source /etc/profile"))?;

        std::fs::write(&marker, "")?;
        log::info!("Crossdev for {chost} complete.");
        Ok(())
    }

    /// Prepare host sandbox for cross-compilation: gcc-16, crossdev overlay,
    /// cross-compiler toolchain. Does NOT configure the sysroot (that's in
    /// sysroot::create). Idempotent via `.crossdev-host-<arch>` marker.
    pub fn prepare_crossdev_host(&self, target_arch: &str, _board: &BoardConfig) -> Result<()> {
        let marker = self.dir.join(format!(".crossdev-host-{target_arch}"));
        if marker.exists() {
            log::info!("Host crossdev for {target_arch} already prepared, skipping.");
            return Ok(());
        }

        let chost = format!("{target_arch}-unknown-linux-gnu");
        let runner = self.runner();

        log::info!("Creating crossdev overlay...");
        runner.run(
            "eselect repository list -i | grep -q crossdev \
             || eselect repository create crossdev",
        )?;

        // Accept unstable packages
        runner.run(&format!(
            "echo 'cross-{chost}/rust-std **' \
             > /etc/portage/package.accept_keywords/rust-std"
        ))?;
        runner.run(
            "echo '<sys-devel/gcc-16.0.9999:16 **' \
             > /etc/portage/package.accept_keywords/gcc",
        )?;
        runner.run("mkdir -p /etc/portage/package.accept_keywords /etc/portage/package.mask")?;

        // Install gcc-16 on host
        log::info!("Emerging gcc:16 (host)...");
        runner.run("emerge -b -k sys-devel/gcc:16")?;
        let gcc_profile = runner.run_output(
            "gcc-config -l | grep '16' | head -n1 | awk '{print $2}'",
        )?;
        runner.run(&format!("gcc-config {gcc_profile}"))?;
        runner.run("source /etc/profile && env-update")?;

        // Install crossdev cross-compiler
        let gcc_ver = runner.run_output(
            "qlist -ICev sys-devel/gcc:16 | head -n1 | sed 's|.*/gcc-||'",
        )?;
        if gcc_ver.is_empty() {
            return Err(Error::CommandFailed {
                code: 1,
                reason: "Could not determine gcc-16 version".into(),
            });
        }
        log::info!("Running crossdev for {chost}...");
        runner.run(&format!(
            "crossdev {chost} --gcc {gcc_ver} \
             --ex-pkg sys-devel/clang-crossdev-wrappers \
             --ex-pkg sys-devel/rust-std"
        ))?;
        runner.run(&format!("gcc-config {chost}-16 && source /etc/profile"))?;

        std::fs::write(&marker, "")?;
        log::info!("Host crossdev for {chost} prepared.");
        Ok(())
    }

    /// Return a `SandboxRunner` for running commands inside this sandbox.
    pub fn runner(&self) -> SandboxRunner {
        SandboxRunner::new(&self.dir)
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

    /// Write portage config files for the cross-sysroot directly on the host fs.
    fn write_crossdev_portage(
        &self,
        portage_dir: &Path,
        arch: &str,
        chost: &str,
        cflags: &str,
        board: &BoardConfig,
    ) -> Result<()> {
        // make.conf for the cross-sysroot
        MakeConf {
            arch,
            chost: Some(chost),
            cflags: Some(cflags),
            mirror: None,
        }
        .write(portage_dir)?;

        for sub in ["env", "package.env", "package.use", "package.accept_keywords"] {
            std::fs::create_dir_all(portage_dir.join(sub))?;
        }

        // env/plain.conf: strip arch-specific flags (used for rust, etc.)
        std::fs::write(
            portage_dir.join("env/plain.conf"),
            "CFLAGS=\"-O3 -pipe\"\nCXXFLAGS=\"-O3 -pipe\"\n",
        )?;

        // package.env
        std::fs::write(portage_dir.join("package.env/rust"), "dev-lang/rust plain.conf\n")?;

        // package.use
        std::fs::write(
            portage_dir.join("package.use/busybox"),
            ">=virtual/libcrypt-2-r1 static-libs\n\
             >=sys-libs/libxcrypt-4.4.36-r3 static-libs\n\
             >=sys-apps/busybox-1.36.1-r3 -pam static\n",
        )?;
        std::fs::write(portage_dir.join("package.use/clang"), "llvm-core/clang -extra\n")?;
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
        for (pkg, flags) in board.workaround_pkgs.iter().zip(board.workaround_cflags.iter()) {
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

/// List all sandbox directories with their state.
pub fn list(ws: &Workspace) -> Result<Vec<SandboxInfo>> {
    let dirs = ws.list_sandboxes()?;
    Ok(dirs
        .into_iter()
        .map(|dir| {
            let arch = crate::workspace::read_arch(&dir)
                .unwrap_or_else(|| "unknown".into());
            let prepared = dir.join(".prepared").exists();
            let name = dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            SandboxInfo { name, arch, prepared }
        })
        .collect())
}

pub struct SandboxInfo {
    pub name: String,
    pub arch: String,
    pub prepared: bool,
}

