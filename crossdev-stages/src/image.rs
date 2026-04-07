use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::board::BoardConfig;
use crate::error::Result;
use crate::portage::Portage;
use crate::sandbox::Sandbox;
use crate::target::Target;
use crate::workspace::Workspace;

/// One timestamped build directory: `builds/<board>-<timestamp>`.
pub struct Build {
    pub dir: PathBuf,
    pub board: String,
}

impl Build {
    /// Create a fresh build directory and write a `.board` marker.
    pub fn create(ws: &Workspace, board: &str) -> Result<Self> {
        let ts = Utc::now().format("%Y%m%dT%H%M%SZ");
        let name = format!("{board}-{ts}");
        let dir = ws.builds_dir().join(&name);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(".board"), board)?;
        Ok(Self { dir, board: board.to_string() })
    }

    /// Open an existing build directory.
    pub fn open(dir: PathBuf) -> Option<Self> {
        let board = std::fs::read_to_string(dir.join(".board"))
            .ok()
            .map(|s| s.trim().to_string())?;
        Some(Self { dir, board })
    }

    fn marker(&self, step: &str) -> PathBuf {
        self.dir.join(format!(".{step}"))
    }

    fn is_done(&self, step: &str) -> bool {
        self.marker(step).exists()
    }

    fn mark_done(&self, step: &str) -> Result<()> {
        std::fs::write(self.marker(step), Utc::now().to_rfc3339())?;
        Ok(())
    }
}

/// Install host-side and target-side packages required for this board.
pub fn install_deps(
    build: &Build,
    sandbox: &Sandbox,
    target: &Target,
    board: &BoardConfig,
    boards_root: &Path,
) -> Result<()> {
    if build.is_done("deps") {
        return Ok(());
    }
    log::info!("[{}] Installing board dependencies…", board.name);

    // Host-side extras from boards/<name>/sandbox-packages.txt
    let sandbox_pkgs = boards_root
        .join(&board.name)
        .join("sandbox-packages.txt");
    if sandbox_pkgs.exists() {
        let runner = sandbox.runner();
        let portage = Portage::new(&runner);
        let content = std::fs::read_to_string(&sandbox_pkgs)?;
        let pkgs: Vec<&str> = content
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        if !pkgs.is_empty() {
            portage.emerge(&pkgs)?;
        }
    }

    // Target-side extras from boards/<name>/target-packages.txt
    let target_pkgs = boards_root
        .join(&board.name)
        .join("target-packages.txt");
    if target_pkgs.exists() {
        target.install_from_file(sandbox, &target_pkgs)?;
    }

    build.mark_done("deps")
}

/// Clone source repositories for bootloader, kernel, and optional firmware.
pub fn checkout(
    build: &Build,
    sandbox: &Sandbox,
    board: &BoardConfig,
    boards_root: &Path,
) -> Result<()> {
    if build.is_done("sources") {
        return Ok(());
    }
    log::info!("[{}] Checking out sources…", board.name);

    // If board.sh defines board_checkout(), delegate to it.
    if board.has_board_sh(boards_root) {
        let _board_sh = boards_root.join(&board.name).join("board.sh");
        let runner = sandbox.runner().with_build(&build.dir, boards_root);
        runner.run(&format!(
            "source /scripts/{name}/board.sh && board_checkout",
            name = board.name
        ))?;
        build.mark_done("sources")?;
        return Ok(());
    }

    let runner = sandbox.runner().with_build(&build.dir, boards_root);

    if let (Some(repo), Some(tag)) = (&board.opensbi_repo, &board.opensbi_tag) {
        runner.run(&format!(
            "git clone --depth=1 --branch {tag} {repo} /build/opensbi"
        ))?;
    }
    if let (Some(repo), Some(tag)) = (&board.u_boot_repo, &board.u_boot_tag) {
        runner.run(&format!(
            "git clone --depth=1 --branch {tag} {repo} /build/u-boot"
        ))?;
    }
    if let Some(repo) = &board.firmware_repo {
        let tag = board.u_boot_tag.as_deref().unwrap_or("main");
        runner.run(&format!(
            "git clone --depth=1 --branch {tag} {repo} /build/firmware"
        ))?;
    }
    runner.run(&format!(
        "git clone --depth=1 --branch {tag} {repo} /build/linux",
        tag = board.kernel_tag,
        repo = board.kernel_repo,
    ))?;

    build.mark_done("sources")
}

/// Compile the bootloader (OpenSBI + U-Boot).
pub fn build_bootloader(
    build: &Build,
    sandbox: &Sandbox,
    board: &BoardConfig,
    boards_root: &Path,
) -> Result<()> {
    if build.is_done("bootloader") {
        return Ok(());
    }
    log::info!("[{}] Building bootloader…", board.name);

    let runner = sandbox.runner().with_build(&build.dir, boards_root);

    if board.has_board_sh(boards_root) {
        runner.run(&format!(
            "source /scripts/{name}/board.sh && board_build_bootloader",
            name = board.name
        ))?;
    } else {
        if let (Some(platform), Some(_repo)) = (&board.opensbi_platform, &board.opensbi_repo) {
            runner.run(&format!(
                "make -C /build/opensbi PLATFORM={platform} \
                 CROSS_COMPILE={cc} -j$(nproc)",
                cc = board.cross_compile,
            ))?;
        }
        if let Some(defconfig) = &board.u_boot_defconfig {
            let karch = board.kernel_arch.as_deref().ok_or_else(|| {
                crate::error::Error::BoardConfigParse {
                    file: board.name.clone(),
                    msg: "KERNEL_ARCH required for bootloader build".into(),
                }
            })?;
            runner.run(&format!(
                "make -C /build/u-boot ARCH={karch} CROSS_COMPILE={cc} {defconfig} && \
                 make -C /build/u-boot ARCH={karch} CROSS_COMPILE={cc} -j$(nproc)",
                cc = board.cross_compile,
            ))?;
        }
    }

    build.mark_done("bootloader")
}

/// Build the Linux kernel.
pub fn build_kernel(
    build: &Build,
    sandbox: &Sandbox,
    board: &BoardConfig,
    boards_root: &Path,
) -> Result<()> {
    if build.is_done("kernel") {
        return Ok(());
    }
    log::info!("[{}] Building kernel…", board.name);

    let runner = sandbox.runner().with_build(&build.dir, boards_root);

    if board.has_board_sh(boards_root) {
        runner.run(&format!(
            "source /scripts/{name}/board.sh && board_build_kernel",
            name = board.name
        ))?;
    } else {
        let karch = board.kernel_arch.as_deref().ok_or_else(|| {
            crate::error::Error::BoardConfigParse {
                file: board.name.clone(),
                msg: "KERNEL_ARCH required for kernel build".into(),
            }
        })?;
        runner.run(&format!(
            "make -C /build/linux ARCH={karch} CROSS_COMPILE={cc} {defconfig} && \
             make -C /build/linux ARCH={karch} CROSS_COMPILE={cc} -j$(nproc)",
            cc = board.cross_compile,
            defconfig = board.kernel_defconfig,
        ))?;
    }

    build.mark_done("kernel")
}

/// Assemble the root filesystem: copy target sysroot, install modules, configure boot.
pub fn assemble(
    build: &Build,
    sandbox: &Sandbox,
    target: &Target,
    board: &BoardConfig,
    boards_root: &Path,
) -> Result<()> {
    if build.is_done("assembled") {
        return Ok(());
    }
    log::info!("[{}] Assembling root filesystem…", board.name);

    let runner = sandbox
        .runner()
        .with_target(&target.dir)
        .with_build(&build.dir, boards_root);

    // Create build directories.
    runner.run("mkdir -p /build/gen/root /build/gen/boot")?;

    // Copy target sysroot.
    runner.run("cp -a /target/. /build/gen/root/")?;

    // Install kernel modules.
    let karch = board.kernel_arch.as_deref().ok_or_else(|| {
        crate::error::Error::BoardConfigParse {
            file: board.name.clone(),
            msg: "KERNEL_ARCH required for modules_install".into(),
        }
    })?;
    runner.run(&format!(
        "make -C /build/linux ARCH={karch} CROSS_COMPILE={cc} \
         INSTALL_MOD_PATH=/build/gen/root modules_install",
        cc = board.cross_compile,
    ))?;

    // Enable services.
    runner.run("mkdir -p /build/gen/root/etc/runlevels/{boot,default,nonetwork,shutdown,sysinit}")?;
    for svc in &board.services {
        if let Some((name, runlevel)) = svc.split_once(':') {
            runner.run(&format!(
                "ln -sf /etc/init.d/{name} /build/gen/root/etc/runlevels/{runlevel}/{name}"
            ))?;
        }
    }

    // Set hostname via /etc/conf.d/hostname (OpenRC style).
    runner.run(&format!(
        "mkdir -p /build/gen/root/etc/conf.d && \
         printf 'hostname=\"{}\"\n' > /build/gen/root/etc/conf.d/hostname",
        board.hostname
    ))?;

    // Configure serial console via inittab.
    if let (Some(tty), Some(baud)) = (&board.serial_tty, &board.serial_baud) {
        runner.run(&format!(
            "echo 'x1:12345:respawn:/sbin/agetty {baud} {tty} linux' \
             >> /build/gen/root/etc/inittab"
        ))?;
    }

    // Clear root password and configure SSH.
    runner.run("sed -i -e 's/root:x:/root::/' /build/gen/root/etc/passwd")?;
    runner.run(
        "mkdir -p /build/gen/root/etc/ssh && \
         printf 'PermitRootLogin yes\nPermitEmptyPasswords yes\nStrictModes yes\n' \
         >> /build/gen/root/etc/ssh/sshd_config",
    )?;

    // Update ldconfig in the assembled rootfs.
    runner.run("/usr/local/bin/ldconfig -v -r /build/gen/root")?;

    // Board-specific assembly overrides (after ldconfig, matching bash script order).
    if board.has_board_sh(boards_root) {
        runner.run(&format!(
            "source /scripts/{name}/board.sh && board_assemble",
            name = board.name
        ))?;
    } else {
        // Default: copy DTBs, firmware overlay, host firmware, kernel image.
        if let (Some(dtb_glob), Some(karch)) = (&board.kernel_dtb_glob, board.kernel_arch.as_deref()) {
            runner.run(&format!(
                "cp /build/linux/arch/{karch}/boot/dts/{dtb_glob} /build/gen/boot/"
            ))?;
        }
        if let (Some(overlay), Some(_)) = (&board.firmware_overlay, &board.firmware_repo) {
            runner.run(&format!(
                "mkdir -p /build/gen/root/lib/firmware && \
                 cp -a /build/firmware/{overlay}/. /build/gen/root/lib/firmware/"
            ))?;
        }
        for hfw in &board.host_firmware_paths {
            runner.run(&format!("cp -a {hfw} /build/gen/root/lib/firmware/"))?;
        }
        if let Some(kname) = &board.kernel_name {
            runner.run(&format!(
                "cp /build/linux/arch/{karch}/boot/{kname} /build/gen/boot/",
            ))?;
        }
    }

    build.mark_done("assembled")
}

/// Pack the assembled rootfs into an image using genimage.
pub fn pack(
    build: &Build,
    sandbox: &Sandbox,
    board: &BoardConfig,
    boards_root: &Path,
) -> Result<()> {
    if build.is_done("packed") {
        return Ok(());
    }
    log::info!("[{}] Packing image…", board.name);

    let runner = sandbox.runner().with_build(&build.dir, boards_root);

    // Find genimage.cfg: board-specific first, then project default.
    let board_cfg = boards_root.join(&board.name).join("genimage.cfg");
    let cfg_path = if board_cfg.exists() {
        format!("/scripts/{}/genimage.cfg", board.name)
    } else {
        "/scripts/genimage.cfg".to_string()
    };

    runner.run(&format!(
        "cd /build && genimage --config {cfg_path} \
         --rootpath /build/rootfs --outputpath /build/images"
    ))?;

    build.mark_done("packed")
}

/// Run the full build pipeline for a board.
pub fn build(
    ws: &Workspace,
    sandbox: &Sandbox,
    target: &Target,
    board: &BoardConfig,
    boards_root: &Path,
    steps: Option<&[String]>,
) -> Result<()> {
    let build = Build::create(ws, &board.name)?;

    let all_steps = board.build_steps.clone();
    let steps_to_run: Vec<&str> = match steps {
        Some(s) => s.iter().map(String::as_str).collect(),
        None => all_steps.iter().map(String::as_str).collect(),
    };

    for step in &steps_to_run {
        match *step {
            "deps" => install_deps(&build, sandbox, target, board, boards_root)?,
            "checkout" => checkout(&build, sandbox, board, boards_root)?,
            "bootloader" => build_bootloader(&build, sandbox, board, boards_root)?,
            "kernel" => build_kernel(&build, sandbox, board, boards_root)?,
            "assemble" => assemble(&build, sandbox, target, board, boards_root)?,
            "pack" => pack(&build, sandbox, board, boards_root)?,
            other => log::warn!("Unknown build step '{}', skipping.", other),
        }
    }

    Ok(())
}
