mod board;
mod container;
mod error;
mod image;
mod portage;
mod sandbox;
mod stage;
mod sysroot;
mod target;
mod workspace;

use std::path::PathBuf;

use clap::builder::styling::{AnsiColor, Styles};
use clap::{Parser, Subcommand};

use error::Result;
use workspace::Workspace;

// Saner default colored style.
const fn cli_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default().bold())
        .usage(AnsiColor::Green.on_default().bold())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Cyan.on_default())
        // optional extras that look nice
        .error(AnsiColor::Red.on_default().bold())
        .valid(AnsiColor::Green.on_default())
        .invalid(AnsiColor::Red.on_default())
}

// ── Top-level CLI ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "crossdev-stages",
    about = "Gentoo-based cross-compilation stage builder",
    styles = cli_styles()
)]
struct Cli {
    /// Path to the project root (where boards/ lives). Defaults to the current directory.
    #[arg(long, global = true, default_value = ".")]
    project_dir: std::path::PathBuf,

    /// Gentoo mirror URL to use for downloads.
    #[arg(long, global = true)]
    mirror: Option<String>,

    /// Override board's SYSROOT (for testing/debug).
    #[arg(long, global = true)]
    sysroot_override: Option<String>,

    /// Show what would be done without executing.
    #[arg(long, global = true)]
    dry_run: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage host build sandboxes.
    #[command(subcommand)]
    Sandbox(SandboxCmd),

    /// Manage target sysroots.
    Target {
        /// Target architecture (overrides .arch marker; defaults to riscv64 for setup).
        #[arg(long, global = true)]
        arch: Option<String>,
        /// Sandbox name (default: most-recently-modified).
        #[arg(long, global = true)]
        sandbox: Option<String>,
        /// Target name (default: most-recently-modified).
        #[arg(long, global = true)]
        target: Option<String>,
        #[command(subcommand)]
        command: TargetCmd,
    },

    /// Manage cross-compilation sysroots.
    #[command(subcommand)]
    Sysroot(SysrootCmd),

    /// Build board images.
    #[command(subcommand)]
    Image(ImageCmd),

    /// List or download Gentoo stage3 tarballs.
    #[command(subcommand)]
    Stages(StagesCmd),

    /// Clean up stale builds, orphan sysroots, and old stage3 tarballs.
    Cleanup {
        /// Remove everything (all builds, sysroots, stages).
        #[arg(long)]
        all: bool,
    },
}

// ── Sandbox subcommands ──────────────────────────────────────────────────────

#[derive(Subcommand)]
enum SandboxCmd {
    /// Download a stage3 and unpack it as a new sandbox.
    Setup {
        /// Host architecture (e.g. riscv64, x86_64, aarch64).
        #[arg(long, default_value = "riscv64")]
        arch: String,
        /// Sandbox name (default: arch-<timestamp>).
        #[arg(long)]
        name: Option<String>,
    },
    /// List all sandboxes.
    List,
    /// Install host build dependencies and configure portage.
    Prepare {
        /// Sandbox name (default: most-recently-modified).
        #[arg(long)]
        name: Option<String>,
    },
    /// Set up the crossdev cross-compiler toolchain inside a sandbox.
    Crossdev {
        /// Target architecture to set up crossdev for.
        #[arg(long, default_value = "riscv64")]
        arch: String,
        /// Board to read BOARD_CFLAGS and workarounds from.
        #[arg(long)]
        board: Option<String>,
        /// Sandbox name (default: most-recently-modified).
        #[arg(long)]
        name: Option<String>,
    },
    /// Open an interactive shell in a sandbox.
    Enter {
        #[arg(long)]
        name: Option<String>,
    },
    /// Run a command inside a sandbox.
    Run {
        #[arg(long)]
        name: Option<String>,
        /// Command string to execute via bash --login -c.
        cmd: Vec<String>,
    },
    /// Remove a sandbox.
    Destroy { name: String },
}

// ── Target subcommands ───────────────────────────────────────────────────────

#[derive(Subcommand)]
enum TargetCmd {
    /// Download a stage3 and create a target sysroot.
    Setup {
        /// Target name (default: arch-<timestamp>).
        #[arg(long)]
        name: Option<String>,
    },
    /// List all target sysroots.
    List,
    /// Bootstrap the target: cross-emerge baselayout → @system → portage.
    Stage1,
    /// Update the target (@world rebuild).
    Update,
    /// Cross-emerge packages into the target.
    Install { packages: Vec<String> },
    /// Update ldconfig cache in the target.
    Ldconfig,
    /// Remove a target.
    Destroy { name: String },
}

// ── Sysroot subcommands ─────────────────────────────────────────────────────

#[derive(Subcommand)]
enum SysrootCmd {
    /// List all sysroots with their CFLAGS.
    List,
    /// Create a sysroot for a board's CFLAGS (stage3 + glibc rebuild).
    Create {
        /// Sysroot name (e.g. rv64gcv_zvl256b).
        name: String,
        /// Board to read CFLAGS from.
        board: String,
        #[arg(long)]
        sandbox: Option<String>,
    },
    /// Remove a sysroot.
    Destroy { name: String },
}

// ── Image subcommands ────────────────────────────────────────────────────────

#[derive(Subcommand)]
enum ImageCmd {
    /// List available boards.
    ListBoards,
    /// Run the image build pipeline for a board.
    Build {
        #[arg(long)]
        board: String,
        #[arg(long)]
        sandbox: Option<String>,
        #[arg(long)]
        target: Option<String>,
        /// Specific steps to run (default: all steps from board.conf).
        steps: Vec<String>,
    },
    /// Remove incomplete builds.
    Prune,
}

// ── Stages subcommands ───────────────────────────────────────────────────────

#[derive(Subcommand)]
enum StagesCmd {
    /// List available stage3 variants for an architecture.
    List {
        #[arg(long, default_value = "riscv64")]
        arch: String,
    },
    /// Download the stage3 for an architecture.
    Fetch {
        #[arg(long, default_value = "riscv64")]
        arch: String,
    },
}

// ── Entry point ──────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter("crossdev_stages=info")
        .init();

    let ws = Workspace::open()?;
    ws.ensure_dirs()?;

    let boards_root = std::fs::canonicalize(cli.project_dir.join("boards"))
        .unwrap_or_else(|_| cli.project_dir.join("boards"));
    let mirror = cli.mirror.as_deref();

    match cli.command {
        // ── Stages ──────────────────────────────────────────────────────────
        Commands::Stages(StagesCmd::List { arch }) => {
            let items = stage::list(&ws.stages_dir(), &arch, mirror).await?;
            for item in items {
                println!("{item}");
            }
        }
        Commands::Stages(StagesCmd::Fetch { arch }) => {
            let path = stage::fetch(&ws.stages_dir(), &arch, mirror).await?;
            println!("{}", path.display());
        }

        // ── Sandbox ──────────────────────────────────────────────────────────
        Commands::Sandbox(SandboxCmd::List) => {
            for s in sandbox::list(&ws)? {
                let state = if s.prepared { "prepared" } else { "unpacked" };
                println!("{:<20} arch={} state={}", s.name, s.arch, state);
            }
        }
        Commands::Sandbox(SandboxCmd::Setup { arch, name }) => {
            let stage_file = stage::fetch(&ws.stages_dir(), &arch, mirror).await?;
            let name = name.unwrap_or_else(|| {
                format!("{arch}-{}", chrono::Utc::now().format("%Y%m%dT%H%M%SZ"))
            });
            sandbox::Sandbox::create(&ws, &name, &arch, &stage_file)?;
            println!("Sandbox '{name}' created.");
        }
        Commands::Sandbox(SandboxCmd::Prepare { name }) => {
            let dir = ws.resolve_sandbox(name.as_deref())?;
            let sb = sandbox::Sandbox::open(dir)?;
            sb.prepare(mirror)?;
        }
        Commands::Sandbox(SandboxCmd::Crossdev { arch, board, name }) => {
            // Load board config for CFLAGS and workarounds, or use a minimal default.
            let board_cfg = if let Some(b) = &board {
                board::load(&boards_root, b)?
            } else {
                default_board_config(&arch)
            };
            ensure_crossdev(&ws, name.as_deref(), &arch, &board_cfg, mirror).await?;
        }
        Commands::Sandbox(SandboxCmd::Enter { name }) => {
            let dir = ws.resolve_sandbox(name.as_deref())?;
            let sb = sandbox::Sandbox::open(dir)?;
            sb.runner().shell()?;
        }
        Commands::Sandbox(SandboxCmd::Run { name, cmd }) => {
            let dir = ws.resolve_sandbox(name.as_deref())?;
            let sb = sandbox::Sandbox::open(dir)?;
            sb.runner().run(&cmd.join(" "))?;
        }
        Commands::Sandbox(SandboxCmd::Destroy { name }) => {
            let dir = ws.sandbox(&name);
            if dir.is_dir() {
                std::fs::remove_dir_all(&dir)?;
                println!("Removed sandbox '{name}'.");
            } else {
                eprintln!("Sandbox '{name}' does not exist.");
            }
        }

        // ── Target ───────────────────────────────────────────────────────────
        Commands::Target { arch, sandbox, target, command } => {
            match command {
                TargetCmd::List => {
                    for t in target::list(&ws)? {
                        let s1 = if t.stage1 { "stage1" } else { "unpacked" };
                        let upd = t.updated.as_deref().unwrap_or("-");
                        println!(
                            "{:<20} arch={} state={} updated={}",
                            t.name, t.arch, s1, upd
                        );
                    }
                }
                TargetCmd::Setup { name } => {
                    let resolved_arch = arch.unwrap_or_else(|| "riscv64".to_string());
                    let stage_file =
                        stage::fetch(&ws.stages_dir(), &resolved_arch, mirror).await?;
                    let name = name.unwrap_or_else(|| {
                        format!(
                            "{resolved_arch}-{}",
                            chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
                        )
                    });
                    target::Target::create(&ws, &name, &resolved_arch, &stage_file)?;
                    println!("Target '{name}' created.");
                    ensure_crossdev(
                        &ws,
                        sandbox.as_deref(),
                        &resolved_arch,
                        &default_board_config(&resolved_arch),
                        mirror,
                    )
                    .await?;
                }
                TargetCmd::Stage1 => {
                    let (tgt, sb) = ensure_target(
                        &ws,
                        target.as_deref(),
                        arch.as_deref(),
                        sandbox.as_deref(),
                        mirror,
                    )
                    .await?;
                    tgt.build_stage1(&sb)?;
                }
                TargetCmd::Update => {
                    let (tgt, sb) = ensure_target(
                        &ws,
                        target.as_deref(),
                        arch.as_deref(),
                        sandbox.as_deref(),
                        mirror,
                    )
                    .await?;
                    tgt.update(&sb)?;
                }
                TargetCmd::Install { packages } => {
                    let (tgt, sb) = ensure_target(
                        &ws,
                        target.as_deref(),
                        arch.as_deref(),
                        sandbox.as_deref(),
                        mirror,
                    )
                    .await?;
                    let pkgs: Vec<&str> = packages.iter().map(String::as_str).collect();
                    tgt.install(&sb, &pkgs)?;
                }
                TargetCmd::Ldconfig => {
                    let (tgt, sb) = ensure_target(
                        &ws,
                        target.as_deref(),
                        arch.as_deref(),
                        sandbox.as_deref(),
                        mirror,
                    )
                    .await?;
                    tgt.update_ldconfig(&sb)?;
                }
                TargetCmd::Destroy { name } => {
                    let dir = ws.target(&name);
                    if dir.is_dir() {
                        std::fs::remove_dir_all(&dir)?;
                        println!("Removed target '{name}'.");
                    } else {
                        eprintln!("Target '{name}' does not exist.");
                    }
                }
            }
        }

        // ── Sysroot ──────────────────────────────────────────────────────────
        Commands::Sysroot(SysrootCmd::List) => {
            for s in sysroot::list(&ws)? {
                println!("{:<25} {:<10} {}", s.name, s.arch, s.cflags);
            }
        }
        Commands::Sysroot(SysrootCmd::Create {
            name,
            board: board_name,
            sandbox,
        }) => {
            let sd = ws.resolve_sandbox(sandbox.as_deref())?;
            let sb = sandbox::Sandbox::open(sd)?;
            let board_cfg = board::load(&boards_root, &board_name)?;
            sysroot::Sysroot::create(&ws, &sb, &name, &board_cfg, mirror).await?;
            sysroot::apply_workarounds(&ws.sysroot(&name), &board_cfg)?;
        }
        Commands::Sysroot(SysrootCmd::Destroy { name }) => {
            sysroot::destroy(&ws, &name)?;
        }

        // ── Image ────────────────────────────────────────────────────────────
        Commands::Image(ImageCmd::ListBoards) => {
            for b in board::list(&boards_root)? {
                let tag = board::load(&boards_root, &b)
                    .map(|c| if c.testing { " [TESTING]" } else { "" })
                    .unwrap_or("");
                println!("{b}{tag}");
            }
        }
        Commands::Image(ImageCmd::Build {
            board: board_name,
            sandbox,
            target,
            steps,
        }) => {
            let board_cfg = board::load(&boards_root, &board_name)?;

            // Sysroot override: CLI flag > env > board.conf
            let sysroot_name = cli
                .sysroot_override
                .or_else(|| std::env::var("CROSSDEV_SYSROOT").ok())
                .unwrap_or_else(|| board_cfg.sysroot.clone());

            let default_steps: Vec<String> = if board_cfg.build_steps.is_empty() {
                [
                    "deps",
                    "checkout",
                    "bootloader",
                    "kernel",
                    "assemble",
                    "pack",
                ]
                .iter()
                .map(|s| s.to_string())
                .collect()
            } else {
                board_cfg.build_steps.clone()
            };
            let steps_to_show = if steps.is_empty() {
                &default_steps
            } else {
                &steps
            };

            if cli.dry_run {
                let tag = if board_cfg.testing { " [TESTING]" } else { "" };
                println!("Board:      {}{tag}", board_cfg.name);
                println!("Arch:       {}", board_cfg.arch);
                println!("CFLAGS:     {}", board_cfg.effective_cflags());
                if let Some(ldflags) = &board_cfg.ldflags {
                    println!("LDFLAGS:    {ldflags}");
                }
                if let Some(rustflags) = &board_cfg.rustflags {
                    println!("RUSTFLAGS:  {rustflags}");
                }
                println!(
                    "Sysroot:    {} ({})",
                    sysroot_name,
                    ws.sysroot(&sysroot_name).display()
                );
                println!(
                    "Steps:      {}",
                    steps_to_show
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                return Ok(());
            }

            // Ensure sandbox exists, is prepared, and has crossdev for this board's arch.
            let sb =
                ensure_crossdev(&ws, sandbox.as_deref(), &board_cfg.arch, &board_cfg, mirror)
                    .await?;

            // Ensure target exists, creating from stage3 if needed.
            let tgt = match ws.resolve_target(target.as_deref()) {
                Ok(td) => target::Target::open(td)?,
                Err(_) => {
                    let name = target.as_deref().unwrap_or(&board_cfg.arch).to_string();
                    tracing::info!("Target '{name}' not found, creating from stage3…");
                    let stage_file =
                        stage::fetch(&ws.stages_dir(), &board_cfg.arch, mirror).await?;
                    target::Target::create(&ws, &name, &board_cfg.arch, &stage_file)?
                }
            };

            // Ensure the target sysroot is bootstrapped (idempotent via .stage1 marker).
            tgt.build_stage1(&sb)?;

            // Resolve sysroot
            let sr = if !sysroot_name.is_empty() {
                Some(sysroot::Sysroot::resolve(&ws, &sysroot_name)?)
            } else {
                None
            };

            let steps_opt = if steps.is_empty() {
                None
            } else {
                Some(steps.as_slice())
            };
            image::build(
                &ws,
                &sb,
                &tgt,
                &board_cfg,
                &boards_root,
                sr.as_ref(),
                steps_opt,
            )?;
        }
        Commands::Image(ImageCmd::Prune) => {
            let builds = ws.list_builds()?;
            let mut pruned = 0;
            for dir in builds {
                let packed = dir.join(".packed").exists();
                if !packed {
                    std::fs::remove_dir_all(&dir)?;
                    pruned += 1;
                }
            }
            println!("Pruned {pruned} incomplete build(s).");
        }

        // ── Cleanup ─────────────────────────────────────────────────────────
        Commands::Cleanup { all } => {
            let mut total = 0usize;

            // 1. Incomplete builds (always)
            let builds = ws.list_builds()?;
            for dir in &builds {
                let dominated = all || !dir.join(".packed").exists();
                if dominated {
                    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                    if cli.dry_run {
                        println!("Would remove build: {name}");
                    } else {
                        std::fs::remove_dir_all(dir)?;
                        println!("Removed build: {name}");
                    }
                    total += 1;
                }
            }

            // 2. Orphan sysroots (not referenced by any board.conf)
            let board_sysroots: std::collections::HashSet<String> = board::list(&boards_root)
                .unwrap_or_default()
                .iter()
                .filter_map(|name| board::load(&boards_root, name).ok())
                .map(|b| b.sysroot)
                .collect();
            for info in sysroot::list(&ws)? {
                let orphan = all || !board_sysroots.contains(&info.name);
                if orphan {
                    if cli.dry_run {
                        println!("Would remove sysroot: {} ({})", info.name, info.cflags);
                    } else {
                        sysroot::destroy(&ws, &info.name)?;
                    }
                    total += 1;
                }
            }

            // 3. Old stage3 tarballs (keep latest per arch, remove rest)
            let stages_dir = ws.stages_dir();
            if stages_dir.is_dir() {
                let mut by_arch: std::collections::HashMap<
                    String,
                    Vec<(PathBuf, std::time::SystemTime)>,
                > = std::collections::HashMap::new();
                for entry in std::fs::read_dir(&stages_dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    let fname = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();
                    // stage3-riscv64-... → arch = riscv64
                    let arch = fname
                        .strip_prefix("stage3-")
                        .and_then(|s| s.split('-').next())
                        .unwrap_or("unknown")
                        .to_string();
                    let mtime = entry
                        .metadata()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .unwrap_or(std::time::UNIX_EPOCH);
                    by_arch.entry(arch).or_default().push((path, mtime));
                }
                for (_arch, mut files) in by_arch {
                    files.sort_by(|a, b| b.1.cmp(&a.1)); // newest first
                    let to_remove = if all {
                        &files[..]
                    } else {
                        files.get(1..).unwrap_or(&[])
                    };
                    for (path, _) in to_remove {
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                        if cli.dry_run {
                            println!("Would remove stage: {name}");
                        } else {
                            std::fs::remove_file(path)?;
                            println!("Removed stage: {name}");
                        }
                        total += 1;
                    }
                }
            }

            if cli.dry_run {
                println!("{total} item(s) would be removed.");
            } else {
                println!("{total} item(s) cleaned up.");
            }
        }
    }

    Ok(())
}

/// Ensure the sandbox exists, is prepared, and has crossdev for `arch`.
/// Auto-creates a sandbox from the host arch stage3 if none is found.
async fn ensure_crossdev(
    ws: &Workspace,
    sandbox_name: Option<&str>,
    arch: &str,
    board_cfg: &board::BoardConfig,
    mirror: Option<&str>,
) -> Result<sandbox::Sandbox> {
    let sd = match ws.resolve_sandbox(sandbox_name) {
        Ok(p) => p,
        Err(_) => {
            let host_arch = std::env::consts::ARCH; // "x86_64"
            tracing::info!("No sandbox found, creating one for {host_arch}…");
            let stage_file = stage::fetch(&ws.stages_dir(), host_arch, mirror).await?;
            let name =
                format!("{host_arch}-{}", chrono::Utc::now().format("%Y%m%dT%H%M%SZ"));
            sandbox::Sandbox::create(ws, &name, host_arch, &stage_file)?;
            ws.resolve_sandbox(None)?
        }
    };
    let sb = sandbox::Sandbox::open(sd)?;
    sb.prepare(mirror)?;
    sb.setup_crossdev(arch, board_cfg)?;
    Ok(sb)
}

/// Ensure the target exists (fetching + unpacking a stage3 if needed) and
/// that the sandbox has crossdev set up for its arch.  Mirrors bash's
/// `ensure_target`.  Returns (Target, Sandbox) ready for use.
async fn ensure_target(
    ws: &Workspace,
    target_name: Option<&str>,
    arch_override: Option<&str>,
    sandbox_name: Option<&str>,
    mirror: Option<&str>,
) -> Result<(target::Target, sandbox::Sandbox)> {
    let (tgt, resolved_arch) = match ws.resolve_target(target_name) {
        Ok(td) => {
            let tgt = target::Target::open(td)?;
            let arch = arch_override
                .map(String::from)
                .unwrap_or_else(|| tgt.arch.clone());
            (tgt, arch)
        }
        Err(_) => {
            let arch = arch_override.ok_or_else(|| {
                error::Error::TargetNotFound(
                    "target not found; specify --arch to create one".into(),
                )
            })?;
            let name = target_name
                .unwrap_or(&format!("{arch}-stage1"))
                .to_string();
            tracing::info!("Target '{name}' not found, creating from stage3…");
            let stage_file = stage::fetch(&ws.stages_dir(), arch, mirror).await?;
            let tgt = target::Target::create(ws, &name, arch, &stage_file)?;
            (tgt, arch.to_string())
        }
    };
    let sb = ensure_crossdev(
        ws,
        sandbox_name,
        &resolved_arch,
        &default_board_config(&resolved_arch),
        mirror,
    )
    .await?;
    Ok((tgt, sb))
}

/// Build a minimal `BoardConfig` when no board is specified for crossdev setup.
fn default_board_config(arch: &str) -> board::BoardConfig {
    board::BoardConfig {
        name: arch.to_string(),
        arch: arch.to_string(),
        sysroot: String::new(),
        cflags: None,
        ldflags: None,
        rustflags: None,
        cross_compile: format!("{arch}-unknown-linux-gnu-"),
        kernel_arch: None,
        opensbi_repo: None,
        opensbi_tag: None,
        opensbi_platform: None,
        u_boot_repo: None,
        u_boot_tag: None,
        u_boot_defconfig: None,
        firmware_repo: None,
        firmware_overlay: None,
        host_firmware_paths: vec![],
        kernel_repo: String::new(),
        kernel_tag: String::new(),
        kernel_defconfig: String::new(),
        kernel_dtb_glob: None,
        dracut_modules: None,
        root_dev: None,
        console: None,
        hostname: "gentoo".into(),
        serial_tty: None,
        serial_baud: None,
        kernel_name: None,
        ramdisk_name: None,
        loglevel: None,
        services: vec![],
        build_steps: vec![],
        workaround_pkgs: vec![],
        workaround_cflags: vec![],
        image_name: None,
        testing: false,
    }
}
