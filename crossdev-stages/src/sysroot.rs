use std::path::{Path, PathBuf};

use crate::board::BoardConfig;
use crate::container::{self, SandboxRunner};
use crate::error::{Error, Result};
use crate::portage;
use crate::sandbox::Sandbox;
use crate::stage;
use crate::workspace::Workspace;

/// A sysroot provides cross-compilation headers and libraries for a specific
/// CFLAGS set. Created by unpacking a stage3 (generic base) and rebuilding
/// glibc with board-specific CFLAGS. Other libraries are used for link-time
/// symbol resolution only; the target rootfs gets its own copies.
///
/// Boards with the same SYSROOT name share a sysroot and its PKGDIR cache.
pub struct Sysroot {
    pub dir: PathBuf,
}

#[derive(Debug)]
pub struct SysrootInfo {
    pub name: String,
    pub arch: String,
    pub cflags: String,
}

impl Sysroot {
    /// Open an existing sysroot by name.
    pub fn resolve(ws: &Workspace, name: &str) -> Result<Self> {
        let dir = ws.sysroot(name);
        if dir.is_dir() && dir.join(".cflags").exists() {
            Ok(Self { dir })
        } else {
            Err(Error::SysrootNotFound(format!(
                "'{name}' not found. Create with: sysroot create {name} <board>"
            )))
        }
    }

    /// Create a new sysroot for a board's CFLAGS.
    ///
    /// 1. Unpack stage3 as generic base
    /// 2. Configure portage with CBUILD, CHOST, ROOT, CFLAGS, PKGDIR
    /// 3. Rebuild glibc with board CFLAGS (the only ABI-critical package)
    pub async fn create(
        ws: &Workspace,
        sb: &Sandbox,
        name: &str,
        board: &BoardConfig,
        mirror: Option<&str>,
    ) -> Result<Self> {
        let dir = ws.sysroot(name);
        if dir.is_dir() && dir.join(".cflags").exists() {
            println!("Sysroot '{name}' already exists at {}", dir.display());
            return Ok(Self { dir });
        }

        let chost = board.chost();
        let cflags = board.effective_cflags();
        let crossdev_root = format!("/usr/{chost}");

        println!("Creating sysroot '{name}' for {chost} with CFLAGS: {cflags}");

        // Ensure host sandbox has crossdev toolchain
        if !sb.dir.join(format!(".crossdev-host-{}", board.arch)).exists() {
            sb.prepare_crossdev_host(&board.arch, board)?;
        }

        // Step 1: Unpack stage3 as sysroot base
        let stage_file = stage::fetch(&ws.stages_dir(), &board.arch, mirror).await?;
        println!("==> Unpacking stage3 into sysroot...");
        container::unpack_tarball(&stage_file, &dir, ws.base())?;

        // Step 2: Configure portage
        println!("==> Configuring portage...");
        configure_sysroot_portage(&dir, &board.arch, &chost, &crossdev_root, &cflags, board, mirror)?;
        write_portage_env(&dir)?;
        apply_workarounds(&dir, board)?;

        // Step 3: Rebuild glibc with board CFLAGS
        println!("==> Rebuilding glibc with CFLAGS: {cflags}");
        let runner = SandboxRunner::new(&sb.dir).with_sysroot(&dir, &chost);
        runner.run(&format!(
            "{chost}-emerge -1 -b -k sys-libs/glibc sys-kernel/linux-headers"
        ))?;

        // Record metadata
        std::fs::write(dir.join(".cflags"), &cflags)?;
        std::fs::write(dir.join(".arch"), &board.arch)?;
        std::fs::write(
            dir.join(".created"),
            chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string(),
        )?;

        println!("Sysroot '{name}' created at {}", dir.display());
        Ok(Self { dir })
    }

}

/// List all sysroots with their metadata.
pub fn list(ws: &Workspace) -> Result<Vec<SysrootInfo>> {
    let sdir = ws.sysroots_dir();
    if !sdir.exists() {
        return Ok(vec![]);
    }
    let mut result = Vec::new();
    for entry in std::fs::read_dir(&sdir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry
            .file_name()
            .into_string()
            .unwrap_or_else(|_| "???".into());
        let cflags = std::fs::read_to_string(path.join(".cflags"))
            .unwrap_or_else(|_| "(unknown)".into())
            .trim()
            .to_string();
        let arch = std::fs::read_to_string(path.join(".arch"))
            .unwrap_or_else(|_| "(unknown)".into())
            .trim()
            .to_string();
        result.push(SysrootInfo { name, arch, cflags });
    }
    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

/// Remove a sysroot directory (via hakoniwa to handle root-owned files).
pub fn destroy(ws: &Workspace, name: &str) -> Result<()> {
    let dir = ws.sysroot(name);
    if !dir.is_dir() {
        return Err(Error::SysrootNotFound(name.into()));
    }
    println!("Removing sysroot: {name}");
    // Use hakoniwa to remove (may contain root-owned files from stage3)
    let parent = ws.sysroots_dir();
    let mut c = hakoniwa::Container::new();
    c.rootfs("/")?
        .runctl(hakoniwa::Runctl::AllowNewPrivs)
        .tmpfsmount("/dev/shm")
        .tmpfsmount("/tmp")
        .bindmount_rw(parent.to_str().unwrap_or_default(), "/target")
        .uidmap(0)
        .gidmap(0);
    let mut cmd = c.command("/bin/sh");
    let rm_cmd = format!("rm -rf /target/{name}");
    cmd.arg("-c").arg(rm_cmd.as_str());
    crate::error::check_status(cmd.status()?)?;
    println!("Sysroot '{name}' removed.");
    Ok(())
}

// ── Internal helpers ────────────────────────────────────────────────────────

fn configure_sysroot_portage(
    sysroot_dir: &Path,
    target_arch: &str,
    chost: &str,
    crossdev_root: &str,
    cflags: &str,
    board: &BoardConfig,
    mirror: Option<&str>,
) -> Result<()> {
    // Set profile via absolute symlink (sysroot has no portage tree)
    let profile = crate::stage::gentoo_profile(target_arch)?;
    let profile_link = sysroot_dir.join("etc/portage/make.profile");
    if profile_link.exists() || profile_link.is_symlink() {
        std::fs::remove_file(&profile_link).or_else(|_| std::fs::remove_dir_all(&profile_link))?;
    }
    std::os::unix::fs::symlink(
        format!("/var/db/repos/gentoo/profiles/{profile}"),
        &profile_link,
    )?;

    // Update make.conf
    let make_conf = sysroot_dir.join("etc/portage/make.conf");
    let host_arch = std::env::consts::ARCH;
    let cbuild = format!("{host_arch}-pc-linux-gnu");
    let cpus = num_cpus::get();
    let jobs = cpus / 2 + 1;

    portage::set_make_conf_var(&make_conf, "CBUILD", &cbuild)?;
    portage::set_make_conf_var(&make_conf, "CHOST", chost)?;
    portage::set_make_conf_var(&make_conf, "ROOT", &format!("{crossdev_root}/"))?;
    portage::set_make_conf_var(&make_conf, "CFLAGS", cflags)?;
    portage::set_make_conf_var(&make_conf, "CXXFLAGS", cflags)?;
    if let Some(ldflags) = &board.ldflags {
        portage::set_make_conf_var(&make_conf, "LDFLAGS", ldflags)?;
    }
    if let Some(rustflags) = &board.rustflags {
        portage::set_make_conf_var(&make_conf, "RUSTFLAGS", rustflags)?;
    }
    portage::set_make_conf_var(
        &make_conf,
        "MAKEOPTS",
        &format!("-j{jobs} --load-average {cpus}"),
    )?;
    portage::set_make_conf_var(
        &make_conf,
        "EMERGE_DEFAULT_OPTS",
        &format!("--jobs={jobs} --load-average {cpus}"),
    )?;
    portage::set_make_conf_var(&make_conf, "FEATURES", "parallel-install -merge-wait")?;
    portage::set_make_conf_var(&make_conf, "PKGDIR", &format!("{crossdev_root}/packages"))?;

    if let Some(llvm_target) = crate::stage::llvm_target(target_arch) {
        portage::set_make_conf_var(&make_conf, "LLVM_TARGETS", llvm_target)?;
    }
    if let Some(m) = mirror {
        portage::set_make_conf_var(&make_conf, "GENTOO_MIRRORS", m)?;
    }

    Ok(())
}

fn write_portage_env(sysroot_dir: &Path) -> Result<()> {
    let portage = sysroot_dir.join("etc/portage");
    std::fs::create_dir_all(portage.join("env"))?;
    std::fs::create_dir_all(portage.join("package.env"))?;
    std::fs::create_dir_all(portage.join("package.use"))?;
    std::fs::create_dir_all(portage.join("package.accept_keywords"))?;

    // Plain CFLAGS for packages that can't handle board flags
    std::fs::write(
        portage.join("env/plain.conf"),
        "CFLAGS=\"-O3 -pipe\"\nCXXFLAGS=\"-O3 -pipe\"\n",
    )?;
    std::fs::write(portage.join("package.env/rust"), "dev-lang/rust plain.conf\n")?;

    // Package USE
    std::fs::write(
        portage.join("package.use/busybox"),
        ">=virtual/libcrypt-2-r1 static-libs\n\
         >=sys-libs/libxcrypt-4.4.36-r3 static-libs\n\
         >=sys-apps/busybox-1.36.1-r3 -pam static\n",
    )?;
    std::fs::write(portage.join("package.use/clang"), "llvm-core/clang -extra\n")?;
    std::fs::write(
        portage.join("package.use/rust"),
        "dev-lang/rust rustfmt -system-llvm\n",
    )?;
    std::fs::write(portage.join("package.use/git"), "dev-vcs/git -iconv\n")?;
    std::fs::write(
        portage.join("package.accept_keywords/gcc"),
        "<sys-devel/gcc-16.0.9999:16 **\n",
    )?;

    // Cross-compile cache
    std::fs::write(
        portage.join("env/cross-cache.conf"),
        "# Force cross-compilation mode for autoconf\n\
         cross_compiling=yes\n\
         ac_cv_func_eventfd=yes\n\
         ac_cv_func_epoll_create1=yes\n\
         ac_cv_func_malloc_0_nonnull=yes\n\
         ac_cv_func_realloc_0_nonnull=yes\n\
         mhd_cv_eventfd_usable=yes\n",
    )?;
    std::fs::write(
        portage.join("package.env/cross-cache"),
        "*/* cross-cache.conf\n",
    )?;

    Ok(())
}

/// Apply per-package CFLAGS workarounds from board config.
pub fn apply_workarounds(sysroot_dir: &Path, board: &BoardConfig) -> Result<()> {
    if board.workaround_pkgs.is_empty() {
        return Ok(());
    }
    if board.workaround_pkgs.len() != board.workaround_cflags.len() {
        return Err(Error::BoardConfigParse {
            file: format!("boards/{}/board.conf", board.name),
            msg: "WORKAROUND_PKGS and WORKAROUND_CFLAGS length mismatch".into(),
        });
    }

    let env_dir = sysroot_dir.join("etc/portage/env");
    let pkg_env_dir = sysroot_dir.join("etc/portage/package.env");
    std::fs::create_dir_all(&env_dir)?;
    std::fs::create_dir_all(&pkg_env_dir)?;

    let mut workaround_lines = Vec::new();
    for (pkg, flags) in board.workaround_pkgs.iter().zip(board.workaround_cflags.iter()) {
        let env_name = pkg.rsplit('/').next().unwrap_or(pkg);
        std::fs::write(
            env_dir.join(format!("{env_name}.conf")),
            format!("CFLAGS=\"{flags}\"\nCXXFLAGS=\"{flags}\"\n"),
        )?;
        workaround_lines.push(format!("{pkg} {env_name}.conf"));
    }
    std::fs::write(
        pkg_env_dir.join("workarounds"),
        workaround_lines.join("\n") + "\n",
    )?;

    println!(
        "Applied {} CFLAGS workaround(s) for {}",
        board.workaround_pkgs.len(),
        board.name,
    );
    Ok(())
}
