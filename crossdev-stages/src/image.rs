use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::Result;
use crate::portage::Portage;
use crate::sandbox::Sandbox;
use crate::sysroot::Sysroot;
use crate::target::Target;
use crate::workspace::Workspace;

/// Derive the project root from `boards_root` (its parent).
/// Bash mounts $BASE_DIR at /scripts, so /scripts/boards/<name>/... works.
fn project_root(boards_root: &Path) -> PathBuf {
    boards_root.parent().unwrap_or(boards_root).to_path_buf()
}

/// One timestamped build directory: `builds/<board>-<timestamp>`.
pub struct Build {
    pub dir: PathBuf,
    pub board: String,
}

impl Build {
    /// Reuse latest incomplete build for this board, or create a fresh one.
    pub fn create(ws: &Workspace, board: &str) -> Result<Self> {
        // Try to reuse latest incomplete (not yet packed) build for this board
        if let Ok(builds) = ws.list_builds() {
            for dir in builds {
                if let Some(b) = Self::open(dir.clone()) {
                    if b.board == board && !b.is_done("packed") {
                        tracing::info!("Resuming build: {}", dir.display());
                        return Ok(b);
                    }
                }
            }
        }
        let ts = Utc::now().format("%Y%m%dT%H%M%SZ");
        let name = format!("{board}-{ts}");
        let dir = ws.builds_dir().join(&name);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(".board"), board)?;
        Ok(Self {
            dir,
            board: board.to_string(),
        })
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
    sysroot: Option<&Sysroot>,
) -> Result<()> {
    if build.is_done("deps") {
        return Ok(());
    }
    tracing::info!("[{}] Installing board dependencies…", board.name);

    // Host-side extras from boards/<name>/sandbox-packages.txt
    let sandbox_pkgs = boards_root.join(&board.name).join("sandbox-packages.txt");
    // Apply sysroot workarounds
    if let Some(sr) = sysroot {
        crate::sysroot::apply_workarounds(&sr.dir, board)?;
    }

    if sandbox_pkgs.exists() {
        let runner = board_runner(sandbox, sysroot, board);
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
    let target_pkgs = boards_root.join(&board.name).join("target-packages.txt");
    if target_pkgs.exists() {
        let content = std::fs::read_to_string(&target_pkgs)?;
        let pkgs: Vec<&str> = content
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        if !pkgs.is_empty() {
            let runner = board_runner(sandbox, sysroot, board).with_target(&target.dir);
            let portage = Portage::new(&runner);
            portage.cross_emerge(&board.chost(), &pkgs)?;
        }
    }

    build.mark_done("deps")
}

/// Clone source repositories for bootloader, kernel, and optional firmware.
pub fn checkout(
    build: &Build,
    sandbox: &Sandbox,
    board: &BoardConfig,
    boards_root: &Path,
    sysroot: Option<&Sysroot>,
) -> Result<()> {
    if build.is_done("sources") {
        return Ok(());
    }
    tracing::info!("[{}] Checking out sources…", board.name);

    // If board.sh defines board_checkout(), delegate to it.
    if board_has_func(boards_root, &board.name, "board_checkout") {
        let runner = board_runner(sandbox, sysroot, board)
            .with_build(&build.dir, &project_root(boards_root));
        runner.run(&board_sh_call(&board.name, "board_checkout"))?;
        build.mark_done("sources")?;
        return Ok(());
    }

    let runner =
        board_runner(sandbox, sysroot, board).with_build(&build.dir, &project_root(boards_root));

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
    sysroot: Option<&Sysroot>,
) -> Result<()> {
    if build.is_done("bootloader") {
        return Ok(());
    }
    tracing::info!("[{}] Building bootloader…", board.name);

    let runner =
        board_runner(sandbox, sysroot, board).with_build(&build.dir, &project_root(boards_root));

    if board_has_func(boards_root, &board.name, "board_build_bootloader") {
        runner.run(&board_sh_call(&board.name, "board_build_bootloader"))?;
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
    sysroot: Option<&Sysroot>,
) -> Result<()> {
    if build.is_done("kernel") {
        return Ok(());
    }
    tracing::info!("[{}] Building kernel…", board.name);

    let runner =
        board_runner(sandbox, sysroot, board).with_build(&build.dir, &project_root(boards_root));

    if board_has_func(boards_root, &board.name, "board_build_kernel") {
        runner.run(&board_sh_call(&board.name, "board_build_kernel"))?;
    } else {
        let karch =
            board
                .kernel_arch
                .as_deref()
                .ok_or_else(|| crate::error::Error::BoardConfigParse {
                    file: board.name.clone(),
                    msg: "KERNEL_ARCH required for kernel build".into(),
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
    sysroot: Option<&Sysroot>,
) -> Result<()> {
    if build.is_done("assembled") {
        return Ok(());
    }
    tracing::info!("[{}] Assembling root filesystem…", board.name);

    let runner = board_runner(sandbox, sysroot, board)
        .with_target(&target.dir)
        .with_build(&build.dir, &project_root(boards_root));

    // Create build directories.
    runner.run("mkdir -p /build/gen/root /build/gen/boot")?;

    // Copy target sysroot.
    runner.run("cp -a /target/. /build/gen/root/")?;

    // Install kernel modules.
    let karch =
        board
            .kernel_arch
            .as_deref()
            .ok_or_else(|| crate::error::Error::BoardConfigParse {
                file: board.name.clone(),
                msg: "KERNEL_ARCH required for modules_install".into(),
            })?;
    runner.run(&format!(
        "make -C /build/linux ARCH={karch} CROSS_COMPILE={cc} \
         INSTALL_MOD_PATH=/build/gen/root modules_install",
        cc = board.cross_compile,
    ))?;

    // Enable services.
    runner
        .run("mkdir -p /build/gen/root/etc/runlevels/{boot,default,nonetwork,shutdown,sysinit}")?;
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
    if board_has_func(boards_root, &board.name, "board_assemble") {
        runner.run(&board_sh_call(&board.name, "board_assemble"))?;
    } else {
        // Default: copy DTBs, firmware overlay, host firmware, kernel image.
        if let (Some(dtb_glob), Some(karch)) =
            (&board.kernel_dtb_glob, board.kernel_arch.as_deref())
        {
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
    sysroot: Option<&Sysroot>,
) -> Result<()> {
    if build.is_done("packed") {
        return Ok(());
    }
    tracing::info!("[{}] Packing image…", board.name);

    let runner =
        board_runner(sandbox, sysroot, board).with_build(&build.dir, &project_root(boards_root));

    // Find genimage.cfg: board-specific first, then project default.
    let board_cfg = boards_root.join(&board.name).join("genimage.cfg");
    let cfg_path = if board_cfg.exists() {
        format!("/scripts/boards/{}/genimage.cfg", board.name)
    } else {
        "/scripts/genimage.cfg".to_string()
    };

    let img_name = board
        .image_name
        .clone()
        .unwrap_or_else(|| format!("gentoo-linux-{}_dev-sdcard.img", board.name));

    runner.run(&format!(
        "rm -rf /build/tmp && cd /build && \
         genimage --config {cfg_path} \
         --inputpath /build --outputpath /build --rootpath /build/gen"
    ))?;

    // Compress with xz
    runner.run(&format!("xz -f -T0 -9 /build/{img_name}"))?;
    println!("Image ready: {}/{img_name}.xz", build.dir.display());

    build.mark_done("packed")
}

/// Shell snippet to source board.conf and board.sh, then call a function
/// only if it's defined.
///
/// board.sh functions expect to call run_with_build() etc. which are bash
/// wrappers from sandbox-stage.sh. Inside the Rust container, these don't
/// exist. We provide stubs that execute the script argument directly
/// (since we're already inside the sandbox with /build mounted).
fn board_sh_call(board_name: &str, func: &str) -> String {
    format!(
        r#"run_with_build() {{ shift 2; eval "$@"; }}
run_with_build_and_source() {{ shift 3; local a; while [[ $# -gt 0 && "$1" != "--" ]]; do shift; done; [[ "$1" == "--" ]] && shift; eval "$@"; }}
export LDCONFIG="/usr/local/bin/ldconfig"
source /scripts/boards/{name}/board.conf && source /scripts/boards/{name}/board.sh && \
if type -t {func} &>/dev/null; then {func}; fi"#,
        name = board_name,
    )
}

/// Check if a board.sh defines a specific function.
fn board_has_func(boards_root: &Path, board_name: &str, func: &str) -> bool {
    let board_sh = boards_root.join(board_name).join("board.sh");
    if !board_sh.exists() {
        return false;
    }
    std::fs::read_to_string(&board_sh)
        .map(|s| s.contains(&format!("{func}()")))
        .unwrap_or(false)
}

/// Return a runner with sysroot bound (if provided).
fn board_runner(
    sandbox: &Sandbox,
    sysroot: Option<&Sysroot>,
    board: &BoardConfig,
) -> SandboxRunner {
    let runner = sandbox.runner();
    if let Some(sr) = sysroot {
        runner.with_sysroot(&sr.dir, &board.chost())
    } else {
        runner
    }
}

/// Run the full build pipeline for a board.
pub fn build(
    ws: &Workspace,
    sandbox: &Sandbox,
    target: &Target,
    board: &BoardConfig,
    boards_root: &Path,
    sysroot: Option<&Sysroot>,
    steps: Option<&[String]>,
) -> Result<()> {
    let bld = Build::create(ws, &board.name)?;

    let default_steps = if board.build_steps.is_empty() {
        vec![
            "deps",
            "checkout",
            "bootloader",
            "kernel",
            "assemble",
            "pack",
        ]
    } else {
        board.build_steps.iter().map(String::as_str).collect()
    };
    let steps_to_run: Vec<&str> = match steps {
        Some(s) => s.iter().map(String::as_str).collect(),
        None => default_steps,
    };

    let total = steps_to_run.len();
    for (i, step) in steps_to_run.iter().enumerate() {
        println!("==> [{}/{}] {}...", i + 1, total, step);
        match *step {
            "deps" => install_deps(&bld, sandbox, target, board, boards_root, sysroot)?,
            "checkout" => checkout(&bld, sandbox, board, boards_root, sysroot)?,
            "bootloader" => build_bootloader(&bld, sandbox, board, boards_root, sysroot)?,
            "kernel" => build_kernel(&bld, sandbox, board, boards_root, sysroot)?,
            "assemble" => assemble(&bld, sandbox, target, board, boards_root, sysroot)?,
            "pack" => pack(&bld, sandbox, board, boards_root, sysroot)?,
            other => tracing::warn!("Unknown build step '{}', skipping.", other),
        }
    }

    Ok(())
}
