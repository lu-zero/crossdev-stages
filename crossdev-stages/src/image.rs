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

// ── Step registry ───────────────────────────────────────────────────────────

/// An image-pipeline step. This is the single source of truth for the
/// pipeline order, the CLI/hook name, and the on-disk completion marker.
///
/// The marker names are historical and intentionally kept unchanged:
/// `checkout` writes `.sources`, `assemble` writes `.assembled`, etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Deps,
    Checkout,
    Bootloader,
    Kernel,
    Assemble,
    Pack,
}

impl Step {
    /// Default pipeline, also used for `board info` and dry-run output.
    pub const ALL: &'static [Step] = &[
        Step::Deps,
        Step::Checkout,
        Step::Bootloader,
        Step::Kernel,
        Step::Assemble,
        Step::Pack,
    ];

    /// Name used by the CLI `--steps` argument and pre/post/override hooks.
    pub fn name(self) -> &'static str {
        match self {
            Step::Deps => "deps",
            Step::Checkout => "checkout",
            Step::Bootloader => "bootloader",
            Step::Kernel => "kernel",
            Step::Assemble => "assemble",
            Step::Pack => "pack",
        }
    }

    /// Completion marker written into the build directory.
    pub fn marker(self) -> &'static str {
        match self {
            Step::Deps => "deps",
            Step::Checkout => "sources",
            Step::Bootloader => "bootloader",
            Step::Kernel => "kernel",
            Step::Assemble => "assembled",
            Step::Pack => "packed",
        }
    }

    /// Parse a CLI/board-config step name; unknown names are handled by the
    /// caller (currently a warning) rather than being rejected here.
    pub fn from_name(name: &str) -> Option<Self> {
        Step::ALL.iter().copied().find(|s| s.name() == name)
    }
}

/// Path of the marker that records the final image filename.
pub fn image_marker(dir: &Utf8Path) -> Utf8PathBuf {
    dir.join(".image")
}

/// Whether a build directory has completed the packing step. Used by
/// `status`, `prune`, and `maint gc`; no `.board` marker required.
pub fn is_packed(dir: &Utf8Path) -> bool {
    dir.join(format!(".{}", Step::Pack.marker())).exists()
}

/// Completion state of a build, derived from its step markers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildState {
    /// Steps whose markers are present, in pipeline order.
    pub done: Vec<Step>,
    /// Filename of the packed image, when available.
    pub image: Option<String>,
}

impl BuildState {
    pub fn is_complete(&self) -> bool {
        self.done.contains(&Step::Pack)
    }
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

    fn marker(&self, step: Step) -> Utf8PathBuf {
        self.dir.join(format!(".{}", step.marker()))
    }

    pub fn is_done(&self, step: Step) -> bool {
        self.marker(step).exists()
    }

    fn mark_done(&self, step: Step) -> Result<()> {
        std::fs::write(self.marker(step), Utc::now().to_rfc3339())?;
        Ok(())
    }

    /// Combined completion state, used by `status` and `logs`.
    pub fn state(&self) -> BuildState {
        BuildState {
            done: Step::ALL
                .iter()
                .copied()
                .filter(|s| self.is_done(*s))
                .collect(),
            image: self.image_name(),
        }
    }

    pub fn image_name(&self) -> Option<String> {
        std::fs::read_to_string(image_marker(&self.dir))
            .ok()
            .map(|s| s.trim().to_string())
    }

    fn set_image_name(&self, name: &str) -> Result<()> {
        std::fs::write(image_marker(&self.dir), name)?;
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
    step: Step,
    build: &Build,
    runner: &SandboxRunner,
    boards_root: &Utf8Path,
    board: &BoardConfig,
    default_fn: impl FnOnce(&SandboxRunner) -> Result<()>,
) -> Result<()> {
    if build.is_done(step) {
        return Ok(());
    }

    let board_dir = boards_root.join(&board.name);
    let step_name = step.name();

    let override_sh = format!("override-{step_name}.sh");
    if board_dir.join(&override_sh).exists() {
        runner.run(&run_board_script(&board.name, &override_sh))?;
        return build.mark_done(step);
    }

    let pre_sh = format!("pre-{step_name}.sh");
    if board_dir.join(&pre_sh).exists() {
        runner.run(&run_board_script(&board.name, &pre_sh))?;
    }

    default_fn(runner)?;

    let post_sh = format!("post-{step_name}.sh");
    if board_dir.join(&post_sh).exists() {
        runner.run(&run_board_script(&board.name, &post_sh))?;
    }

    build.mark_done(step)
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

    build.set_image_name(&final_name)?;
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn board_runner(sandbox: &Sandbox, board: &BoardConfig) -> SandboxRunner {
    let _ = board; // arch available if needed later
    sandbox.runner()
}

// ── Pipeline ────────────────────────────────────────────────────────────────

/// Dispatch the default implementation of a known step.
fn default_for_step(
    step: Step,
    runner: &SandboxRunner,
    build: &Build,
    sandbox: &Sandbox,
    target: &Target,
    board: &BoardConfig,
    boards_root: &Utf8Path,
) -> Result<()> {
    match step {
        Step::Deps => default_deps(runner, sandbox, target, board, boards_root),
        Step::Checkout => default_checkout(runner, board),
        Step::Bootloader => default_bootloader(runner, board),
        Step::Kernel => default_kernel(runner, board),
        Step::Assemble => default_assemble(runner, board),
        Step::Pack => default_pack(runner, board, build, boards_root),
    }
}

pub fn build(
    ws: &Workspace,
    sandbox: &Sandbox,
    target: &Target,
    board: &BoardConfig,
    boards_root: &Utf8Path,
    steps: Option<&[String]>,
) -> Result<()> {
    let bld = Build::create(ws, &board.name)?;

    let default_steps: Vec<String> = if board.build_steps.is_empty() {
        Step::ALL.iter().map(|s| s.name().to_string()).collect()
    } else {
        board.build_steps.clone()
    };
    let steps_to_run: Vec<&str> = match steps {
        Some(s) => s.iter().map(String::as_str).collect(),
        None => default_steps.iter().map(String::as_str).collect(),
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

        let result = match Step::from_name(step) {
            Some(s) => run_step(s, &bld, &runner, boards_root, board, |r| {
                default_for_step(s, r, &bld, sandbox, target, board, boards_root)
            }),
            None => {
                tracing::warn!("Unknown step '{}', skipping.", step);
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

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(Utf8PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = Utf8PathBuf::try_from(std::env::temp_dir().join(format!(
                "crossdev-stages-build-{tag}-{}",
                std::process::id()
            )))
            .unwrap();
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn build(&self) -> Build {
            Build {
                dir: self.0.clone(),
                board: "test".into(),
            }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn step_names_and_markers_are_stable() {
        // Locks the historical names/markers, including the irregular ones.
        let expected = [
            (Step::Deps, "deps", "deps"),
            (Step::Checkout, "checkout", "sources"),
            (Step::Bootloader, "bootloader", "bootloader"),
            (Step::Kernel, "kernel", "kernel"),
            (Step::Assemble, "assemble", "assembled"),
            (Step::Pack, "pack", "packed"),
        ];
        assert_eq!(Step::ALL.len(), expected.len());
        for (step, name, marker) in expected {
            assert_eq!(step.name(), name);
            assert_eq!(step.marker(), marker);
            assert_eq!(Step::from_name(name), Some(step));
        }
        assert_eq!(Step::from_name("bogus"), None);
    }

    #[test]
    fn build_state_tracks_markers_in_pipeline_order() {
        let dir = TempDir::new("state");
        let build = dir.build();
        assert!(build.state().done.is_empty());
        assert!(!build.state().is_complete());

        build.mark_done(Step::Assemble).unwrap();
        build.mark_done(Step::Deps).unwrap();
        assert_eq!(build.state().done, vec![Step::Deps, Step::Assemble]);
        assert!(!build.state().is_complete());

        build.mark_done(Step::Pack).unwrap();
        assert!(build.state().is_complete());
        assert!(is_packed(&build.dir));
    }

    #[test]
    fn image_name_roundtrip() {
        let dir = TempDir::new("image");
        let build = dir.build();
        assert_eq!(build.image_name(), None);
        build.set_image_name("image.img.xz\n").unwrap();
        assert_eq!(build.image_name(), Some("image.img.xz".into()));
        assert_eq!(
            std::fs::read_to_string(image_marker(&build.dir)).unwrap(),
            "image.img.xz\n"
        );
    }
}
