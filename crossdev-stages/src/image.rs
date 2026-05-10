use camino::{Utf8Path, Utf8PathBuf};

use chrono::Utc;

use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::Result;
use crate::portage::Portage;
use crate::sandbox::Sandbox;
use crate::target::Target;
use crate::workspace::Workspace;

fn project_root(boards_root: &Utf8Path) -> Utf8PathBuf {
    boards_root.parent().unwrap_or(boards_root).to_path_buf()
}

// ── Build directory ─────────────────────────────────────────────────────────

pub struct Build {
    pub dir: Utf8PathBuf,
    pub board: String,
}

impl Build {
    pub fn create(ws: &Workspace, board: &str) -> Result<Self> {
        // One build directory per board.  Reuse it across re-runs: the
        // timestamp written at first create stays put, so a re-pack
        // overwrites the same `*-<ts>.img.xz` instead of accumulating
        // copies.  To start over (and pick up a fresh timestamp), prune
        // the board's build dir first.
        let dir = ws.builds_dir().join(board);
        if let Some(b) = Self::open(dir.clone()) {
            tracing::info!("Reusing build: {}", dir);
            return Ok(b);
        }
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(".board"), board)?;
        let ts = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        std::fs::write(dir.join(".timestamp"), &ts)?;
        Ok(Self {
            dir,
            board: board.to_string(),
        })
    }

    pub fn open(dir: Utf8PathBuf) -> Option<Self> {
        let board = std::fs::read_to_string(dir.join(".board"))
            .ok()
            .map(|s| s.trim().to_string())?;
        Some(Self { dir, board })
    }

    /// Wall-clock build timestamp embedded in the produced image filename
    /// (stable across resume — written once at create).
    pub fn timestamp(&self) -> String {
        std::fs::read_to_string(self.dir.join(".timestamp"))
            .ok()
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| Utc::now().format("%Y%m%dT%H%M%SZ").to_string())
    }

    fn marker(&self, step: &str) -> Utf8PathBuf {
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

// ── Step runner with file-convention hooks ───────────────────────────────────
//
// For each step, check boards/<name>/ for:
//   override-{step}.sh  →  replaces Rust default entirely
//   pre-{step}.sh       →  runs before Rust default
//   post-{step}.sh      →  runs after Rust default

fn run_step(
    step: &str,
    marker: &str,
    build: &Build,
    runner: &SandboxRunner,
    boards_root: &Utf8Path,
    board: &BoardConfig,
    default_fn: impl FnOnce(&SandboxRunner) -> Result<()>,
) -> Result<()> {
    if build.is_done(marker) {
        return Ok(());
    }

    let board_dir = boards_root.join(&board.name);

    let override_sh = format!("override-{step}.sh");
    if board_dir.join(&override_sh).exists() {
        runner.run(&run_board_script(&board.name, &override_sh))?;
        return build.mark_done(marker);
    }

    let pre_sh = format!("pre-{step}.sh");
    if board_dir.join(&pre_sh).exists() {
        runner.run(&run_board_script(&board.name, &pre_sh))?;
    }

    default_fn(runner)?;

    let post_sh = format!("post-{step}.sh");
    if board_dir.join(&post_sh).exists() {
        runner.run(&run_board_script(&board.name, &post_sh))?;
    }

    build.mark_done(marker)
}

fn run_board_script(board_name: &str, script: &str) -> String {
    format!(
        "set -e\nexport LDCONFIG=/usr/local/bin/ldconfig\n\
         source /scripts/boards/{name}/board.conf\n\
         source /scripts/boards/{name}/{script}",
        name = board_name,
    )
}

// ── Default implementations ─────────────────────────────────────────────────

fn default_deps(
    _runner: &SandboxRunner,
    sandbox: &Sandbox,
    target: &Target,
    board: &BoardConfig,
    boards_root: &Utf8Path,
) -> Result<()> {
    let sandbox_pkgs = boards_root.join(&board.name).join("sandbox-packages.txt");
    if sandbox_pkgs.exists() {
        let host_runner = board_runner(sandbox, board);
        let portage = Portage::new(&host_runner);
        let content = std::fs::read_to_string(&sandbox_pkgs)?;
        let lines: Vec<&str> = content
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();

        // Write package.accept_keywords entries for packages with keyword overrides.
        // Format: "atom [keywords]" — e.g. "sys-boot/syslinux **" or "dev-libs/foo ~amd64"
        let accept_keywords_dir = sandbox.dir.join("etc/portage/package.accept_keywords");
        std::fs::create_dir_all(&accept_keywords_dir)?;
        let mut pkgs: Vec<&str> = Vec::new();
        for line in &lines {
            let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
            let atom = parts[0];
            pkgs.push(atom);
            if parts.len() > 1 {
                let keywords = parts[1].trim();
                let safe_name = atom.replace('/', "_");
                std::fs::write(
                    accept_keywords_dir.join(&safe_name),
                    format!("{atom} {keywords}\n"),
                )?;
            }
        }

        // Write package.use entries from sandbox-packages.use.
        // Format: "atom use_flags..." — e.g. "sys-boot/syslinux bios -uefi"
        let sandbox_use = boards_root.join(&board.name).join("sandbox-packages.use");
        if sandbox_use.exists() {
            let use_dir = sandbox.dir.join("etc/portage/package.use");
            std::fs::create_dir_all(&use_dir)?;
            let use_content = std::fs::read_to_string(&sandbox_use)?;
            for line in use_content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
                let atom = parts[0];
                let safe_name = atom.replace('/', "_");
                std::fs::write(use_dir.join(&safe_name), format!("{line}\n"))?;
            }
        }

        if !pkgs.is_empty() {
            portage.emerge(&pkgs)?;
        }
    }

    let target_pkgs = boards_root.join(&board.name).join("target-packages.txt");
    if target_pkgs.exists() {
        let content = std::fs::read_to_string(&target_pkgs)?;
        let pkgs: Vec<&str> = content
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        if !pkgs.is_empty() {
            let target_runner = board_runner(sandbox, board).with_target(&target.dir);
            let portage = Portage::new(&target_runner);
            portage.cross_emerge(&board.chost(), &pkgs)?;
        }
    }

    Ok(())
}

fn default_checkout(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    crate::bootloader::clone_pipeline(runner, board)?;
    if let Some(repo) = &board.firmware_repo {
        let tag = board.firmware_tag.as_deref().unwrap_or("master");
        crate::source_cache::cached_clone(runner, repo, tag, "/build/firmware", "firmware")?;
    }
    crate::source_cache::cached_clone(
        runner,
        &board.kernel_repo,
        &board.kernel_tag,
        "/build/linux",
        &format!("linux-{}", board.name),
    )
}

fn default_bootloader(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    crate::bootloader::build_pipeline(runner, board)
}

fn default_kernel(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
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
         make -C /build/linux ARCH={karch} CROSS_COMPILE={cc} WERROR=0 -j$(nproc)",
        cc = board.cross_compile,
        defconfig = board.kernel_defconfig,
    ))
}

fn default_assemble(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    let karch =
        board
            .kernel_arch
            .as_deref()
            .ok_or_else(|| crate::error::Error::BoardConfigParse {
                file: board.name.clone(),
                msg: "KERNEL_ARCH required for assemble".into(),
            })?;

    runner.run("mkdir -p /build/gen/root /build/gen/boot")?;
    runner.run("cp -a /target/. /build/gen/root/")?;
    // unpack_tarball excludes ./dev to avoid permission errors in rootless containers.
    // Recreate the empty mount-point directories so the kernel can mount devtmpfs,
    // procfs, sysfs and tmpfs at boot.
    runner.run("mkdir -p /build/gen/root/{dev,proc,sys,run,tmp}")?;

    runner.run(&format!(
        "make -C /build/linux ARCH={karch} CROSS_COMPILE={cc} \
         INSTALL_MOD_PATH=/build/gen/root modules_install",
        cc = board.cross_compile,
    ))?;

    if let Some(dtb_glob) = &board.kernel_dtb_glob {
        runner.run(&format!("cp /build/linux/{dtb_glob} /build/gen/boot/"))?;
    }

    if let Some(kname) = &board.kernel_name {
        runner.run(&format!(
            "cp /build/linux/arch/{karch}/boot/{kname} /build/gen/boot/"
        ))?;
    }

    runner
        .run("mkdir -p /build/gen/root/etc/runlevels/{boot,default,nonetwork,shutdown,sysinit}")?;

    // Board-agnostic: grow-rootfs oneshot, runs once on first boot, fills
    // the rootfs partition out to the disk end + resize2fs.  Needs
    // sys-block/parted + sys-fs/e2fsprogs in the target.
    runner.run(
        "install -m 0755 /scripts/defaults/scripts/grow-rootfs.initd \
           /build/gen/root/etc/init.d/grow-rootfs && \
         ln -sf /etc/init.d/grow-rootfs \
           /build/gen/root/etc/runlevels/boot/grow-rootfs",
    )?;

    for svc in &board.services {
        if let Some((name, runlevel)) = svc.split_once(':') {
            runner.run(&format!(
                "ln -sf /etc/init.d/{name} /build/gen/root/etc/runlevels/{runlevel}/{name}"
            ))?;
        }
    }

    runner.run(&format!(
        "mkdir -p /build/gen/root/etc/conf.d && \
         printf 'hostname=\"{}\"\n' > /build/gen/root/etc/conf.d/hostname",
        board.hostname
    ))?;

    if let (Some(tty), Some(baud)) = (&board.serial_tty, &board.serial_baud) {
        runner.run(&format!(
            "echo 'x1:12345:respawn:/sbin/agetty {baud} {tty} linux' \
             >> /build/gen/root/etc/inittab"
        ))?;
    }

    runner.run("sed -i -e 's/root:x:/root::/' /build/gen/root/etc/passwd")?;
    runner.run(
        "mkdir -p /build/gen/root/etc/ssh && \
         printf 'PermitRootLogin yes\nPermitEmptyPasswords yes\nStrictModes yes\n' \
         >> /build/gen/root/etc/ssh/sshd_config",
    )?;

    if let Some(dracut_modules) = &board.dracut_modules {
        runner.run(&format!(
            "kver=$(ls /build/gen/root/lib/modules/ | head -1) && \
             [ -n \"$kver\" ] && \
             dracutbasedir=/usr/lib/dracut \
             DRACUT_INSTALL=/usr/lib/dracut/dracut-install \
               dracut -f --no-early-microcode --no-kernel \
                 -m '{dracut_modules}' --gzip \
                 --sysroot /build/gen/root \
                 --tmpdir /tmp \
                 /build/gen/boot/initramfs.img \"$kver\""
        ))?;
    }

    runner.run("/usr/local/bin/ldconfig -v -r /build/gen/root")
}

fn default_pack(
    runner: &SandboxRunner,
    board: &BoardConfig,
    build: &Build,
    boards_root: &Utf8Path,
) -> Result<()> {
    let board_cfg = boards_root.join(&board.name).join("genimage.cfg");
    let cfg_path = if board_cfg.exists() {
        format!("/scripts/boards/{}/genimage.cfg", board.name)
    } else {
        "/scripts/genimage.cfg".to_string()
    };

    let cfg_name = board
        .image_name
        .clone()
        .unwrap_or_else(|| format!("gentoo-linux-{}_dev-sdcard.img", board.name));

    runner.run(&format!(
        "rm -rf /build/tmp && cd /build && \
         genimage --config {cfg_path} \
         --mkdosfs mkfs.vfat \
         --inputpath /build --outputpath /build --rootpath /build/gen"
    ))?;

    // Stamp the build timestamp into the image filename so successive builds
    // don't shadow each other when the user copies them out, e.g.
    //   gentoo-linux-premier-p550_dev-sdcard-20260622T031736Z.img.xz
    let ts = build.timestamp();
    let img_name = match cfg_name.strip_suffix(".img") {
        Some(stem) => format!("{stem}-{ts}.img"),
        None => format!("{cfg_name}-{ts}"),
    };
    runner.run(&format!("mv /build/{cfg_name} /build/{img_name}"))?;

    let compression = board.compression.as_deref().unwrap_or("xz");
    let final_name = match compression {
        "none" => {
            println!("Image ready: {}/{img_name}", build.dir);
            img_name.clone()
        }
        "gz" | "gzip" => {
            runner.run(&format!("gzip -fv -9 /build/{img_name}"))?;
            let name = format!("{img_name}.gz");
            println!("Image ready: {}/{name}", build.dir);
            name
        }
        _ => {
            runner.run(&format!("xz -fv -T0 -9 /build/{img_name}"))?;
            let name = format!("{img_name}.xz");
            println!("Image ready: {}/{name}", build.dir);
            name
        }
    };

    std::fs::write(build.dir.join(".image"), &final_name)?;
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn board_runner(sandbox: &Sandbox, board: &BoardConfig) -> SandboxRunner {
    let _ = board; // arch available if needed later
    sandbox.runner()
}

// ── Pipeline ────────────────────────────────────────────────────────────────

pub fn build(
    ws: &Workspace,
    sandbox: &Sandbox,
    target: &Target,
    board: &BoardConfig,
    boards_root: &Utf8Path,
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
    let build_start = std::time::Instant::now();

    for (i, step) in steps_to_run.iter().enumerate() {
        let step_start = std::time::Instant::now();
        println!("==> [{}/{}] {}...", i + 1, total, step);

        let runner = board_runner(sandbox, board)
            .with_target(&target.dir)
            .with_build(&bld.dir, &project_root(boards_root))
            .with_cache(ws.base());

        let result = match *step {
            "deps" => run_step("deps", "deps", &bld, &runner, boards_root, board, |_r| {
                default_deps(_r, sandbox, target, board, boards_root)
            }),
            "checkout" => run_step(
                "checkout",
                "sources",
                &bld,
                &runner,
                boards_root,
                board,
                |r| default_checkout(r, board),
            ),
            "bootloader" => run_step(
                "bootloader",
                "bootloader",
                &bld,
                &runner,
                boards_root,
                board,
                |r| default_bootloader(r, board),
            ),
            "kernel" => run_step("kernel", "kernel", &bld, &runner, boards_root, board, |r| {
                default_kernel(r, board)
            }),
            "assemble" => run_step(
                "assemble",
                "assembled",
                &bld,
                &runner,
                boards_root,
                board,
                |r| default_assemble(r, board),
            ),
            "pack" => run_step("pack", "packed", &bld, &runner, boards_root, board, |r| {
                default_pack(r, board, &bld, boards_root)
            }),
            other => {
                tracing::warn!("Unknown step '{}', skipping.", other);
                Ok(())
            }
        };

        let elapsed = step_start.elapsed();
        println!("    {} done ({})", step, format_duration(elapsed));
        result?;
    }

    let total_elapsed = build_start.elapsed();
    println!("\nBuild complete: {}", format_duration(total_elapsed));
    Ok(())
}

fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m {}s", secs / 3600, (secs % 3600) / 60, secs % 60)
    }
}
