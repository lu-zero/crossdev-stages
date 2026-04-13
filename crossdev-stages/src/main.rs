mod board;
mod bootloader;
mod container;
mod error;
mod image;
mod portage;
mod sandbox;
mod source_cache;
mod stage;
mod sysroot;
mod target;
mod workspace;

use camino::Utf8PathBuf;

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
        /// Target architecture (overrides .arch marker; required for setup/create).
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

    /// Show build output and logs.
    Logs {
        /// Board name (shows latest build).
        board: String,
        /// Show only a specific step's output.
        #[arg(long)]
        step: Option<String>,
    },

    /// Export build artifacts to a directory.
    Export {
        /// Board name.
        board: String,
        /// Output directory (default: current directory).
        #[arg(long, short)]
        output: Option<Utf8PathBuf>,
        /// Export all files, not just the final image.
        #[arg(long)]
        all: bool,
    },

    /// Show resolved board configuration.
    Config {
        /// Board name.
        board: String,
    },

    /// Check environment for common issues.
    Doctor,

    /// Show overview of sandboxes, sysroots, builds, and boards.
    Status {
        /// Machine-readable TAB-separated output.
        #[arg(long)]
        tsv: bool,
    },
}

// ── Sandbox subcommands ──────────────────────────────────────────────────────

#[derive(Subcommand)]
enum SandboxCmd {
    /// Download a stage3 and unpack it as a new sandbox.
    Setup {
        /// Host architecture (e.g. x86_64, aarch64, riscv64). Defaults to the current host arch.
        #[arg(long, default_value = std::env::consts::ARCH)]
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
        /// Target architecture to set up crossdev for (e.g. riscv64, aarch64).
        #[arg(long)]
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
        /// Compression: xz (default), gz, none.
        #[arg(long)]
        compression: Option<String>,
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
        #[arg(long, default_value = std::env::consts::ARCH)]
        arch: String,
    },
    /// Download the stage3 for an architecture.
    Fetch {
        #[arg(long, default_value = std::env::consts::ARCH)]
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

    let boards_root = {
        let p = std::fs::canonicalize(cli.project_dir.join("boards"))
            .unwrap_or_else(|_| cli.project_dir.join("boards"));
        Utf8PathBuf::try_from(p).expect("boards path is not UTF-8")
    };
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
            println!("{path}");
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
            sandbox::destroy(&ws, &name)?;
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
                    let resolved_arch = arch.ok_or_else(|| {
                        anyhow::anyhow!("--arch is required for target setup")
                    })?;
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
                    target::destroy(&ws, &name)?;
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
            compression,
            steps,
        }) => {
            let mut board_cfg = board::load(&boards_root, &board_name)?;
            if let Some(c) = compression {
                board_cfg.compression = Some(c);
            }

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
                    ws.sysroot(&sysroot_name)
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
                    let name = dir.file_name().unwrap_or("?");
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
                    Vec<(Utf8PathBuf, std::time::SystemTime)>,
                > = std::collections::HashMap::new();
                for entry in std::fs::read_dir(&stages_dir)? {
                    let entry = entry?;
                    let path = match Utf8PathBuf::try_from(entry.path()) {
                        Ok(p) => p,
                        Err(_) => continue,
                    };
                    if !path.is_file() {
                        continue;
                    }
                    let fname = path.file_name().unwrap_or("").to_string();
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
                        let name = path.file_name().unwrap_or("?");
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

        // ── Logs ────────────────────────────────────────────────────────────
        Commands::Logs { board, step } => {
            let builds = ws.list_builds()?;
            let build = builds
                .iter()
                .filter_map(|dir| image::Build::open(dir.clone()))
                .find(|b| b.board == board)
                .ok_or_else(|| error::Error::BoardNotFound(format!("no builds for '{board}'")))?;

            println!("Build: {}", build.dir);
            println!("Board: {}", build.board);

            let steps = ["deps", "sources", "bootloader", "kernel", "assembled", "packed"];
            for s in &steps {
                let marker = build.dir.join(format!(".{s}"));
                if marker.exists() {
                    let ts = std::fs::read_to_string(&marker).unwrap_or_default();
                    let label = if step.as_deref() == Some(s) { " <--" } else { "" };
                    println!("  {s}: {}{label}", ts.trim());
                }
            }

            let log_dir = ws.logs_dir();
            if let Some(ref step_name) = step {
                let pattern = format!("{}-", board);
                let mut found = false;
                if log_dir.is_dir() {
                    for entry in std::fs::read_dir(&log_dir)? {
                        let entry = entry?;
                        let name = entry.file_name().into_string().unwrap_or_default();
                        if name.contains(&pattern) && name.contains(step_name) {
                            println!("\n--- {} ---", name);
                            let content = std::fs::read_to_string(entry.path())?;
                            print!("{content}");
                            found = true;
                        }
                    }
                }
                if !found {
                    println!("\nNo log files found for step '{step_name}'.");
                    println!("Portage logs may be at: {}/portage/", ws.logs_dir());
                }
            }
        }

        // ── Export ──────────────────────────────────────────────────────────
        Commands::Export { board: board_name, output, all } => {
            let builds = ws.list_builds()?;
            let build = builds
                .iter()
                .filter_map(|dir| image::Build::open(dir.clone()))
                .find(|b| b.board == board_name)
                .ok_or_else(|| error::Error::BoardNotFound(format!("no builds for '{board_name}'")))?;

            let out_dir = output.unwrap_or_else(|| Utf8PathBuf::from("."));
            std::fs::create_dir_all(&out_dir)?;

            if all {
                let mut exported = 0;
                for entry in std::fs::read_dir(&build.dir)? {
                    let entry = entry?;
                    let name = entry.file_name().into_string().unwrap_or_default();
                    if name.starts_with('.') || !entry.path().is_file() {
                        continue;
                    }
                    let dest = out_dir.join(&name);
                    std::fs::copy(entry.path(), &dest)?;
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    println!("{name} ({:.1}M)", size as f64 / 1_048_576.0);
                    exported += 1;
                }
                println!("{exported} file(s) exported to {out_dir}");
            } else {
                let image_marker = build.dir.join(".image");
                let img_name = std::fs::read_to_string(&image_marker)
                    .map(|s| s.trim().to_string())
                    .ok();

                if let Some(name) = img_name {
                    let src = build.dir.join(&name);
                    if src.is_file() {
                        let dest = out_dir.join(&name);
                        std::fs::copy(&src, &dest)?;
                        let size = std::fs::metadata(&src).map(|m| m.len()).unwrap_or(0);
                        println!("{name} ({:.1}M) -> {dest}", size as f64 / 1_048_576.0);
                    } else {
                        println!("Image file missing: {src}");
                    }
                } else {
                    println!("Build not packed yet. Run: crossdev-stages image build --board {board_name}");
                }
            }
        }

        // ── Config ──────────────────────────────────────────────────────────
        Commands::Config { board: board_name } => {
            let board_cfg = board::load(&boards_root, &board_name)?;
            println!("Board:          {}", board_cfg.name);
            println!("Arch:           {}", board_cfg.arch);
            println!("CHOST:          {}", board_cfg.chost());
            println!("CFLAGS:         {}", board_cfg.effective_cflags());
            println!("Sysroot:        {}", board_cfg.sysroot);
            println!("Cross-compile:  {}", board_cfg.cross_compile);
            if let Some(k) = &board_cfg.kernel_arch { println!("Kernel arch:    {k}"); }
            println!("Kernel repo:    {}", board_cfg.kernel_repo);
            println!("Kernel tag:     {}", board_cfg.kernel_tag);
            println!("Kernel defconf: {}", board_cfg.kernel_defconfig);
            if let Some(r) = &board_cfg.opensbi_repo { println!("OpenSBI repo:   {r}"); }
            if let Some(t) = &board_cfg.opensbi_tag { println!("OpenSBI tag:    {t}"); }
            if let Some(p) = &board_cfg.opensbi_platform { println!("OpenSBI plat:   {p}"); }
            if let Some(f) = &board_cfg.opensbi_fw_type { println!("OpenSBI fw:     {f}"); }
            if let Some(f) = &board_cfg.opensbi_make_flags { println!("OpenSBI flags:  {f}"); }
            if let Some(r) = &board_cfg.u_boot_repo { println!("U-Boot repo:    {r}"); }
            if let Some(t) = &board_cfg.u_boot_tag { println!("U-Boot tag:     {t}"); }
            if let Some(d) = &board_cfg.u_boot_defconfig { println!("U-Boot deconf:  {d}"); }
            if let Some(f) = &board_cfg.u_boot_make_flags { println!("U-Boot flags:   {f}"); }
            if !board_cfg.build_steps.is_empty() {
                println!("Build steps:    {}", board_cfg.build_steps.join(" "));
            }
            if board_cfg.testing { println!("Testing:        yes"); }

            // Show hook scripts
            let board_dir = boards_root.join(&board_name);
            let steps = ["deps", "checkout", "bootloader", "kernel", "assemble", "pack"];
            let mut hooks = Vec::new();
            for s in &steps {
                if board_dir.join(format!("override-{s}.sh")).exists() {
                    hooks.push(format!("override-{s}.sh"));
                }
                if board_dir.join(format!("pre-{s}.sh")).exists() {
                    hooks.push(format!("pre-{s}.sh"));
                }
                if board_dir.join(format!("post-{s}.sh")).exists() {
                    hooks.push(format!("post-{s}.sh"));
                }
            }
            if !hooks.is_empty() {
                println!("Hooks:          {}", hooks.join(", "));
            }
        }

        // ── Doctor ──────────────────────────────────────────────────────────
        Commands::Doctor => {
            let mut ok = 0;
            let mut fail = 0;

            macro_rules! check {
                ($label:expr, $cond:expr) => {
                    if $cond {
                        println!("  [ok] {}", $label);
                        ok += 1;
                    } else {
                        println!("  [!!] {}", $label);
                        fail += 1;
                    }
                };
            }

            println!("Workspace: {}", ws.base());
            check!("stages dir", ws.stages_dir().is_dir());
            check!("sandboxes dir", ws.sandboxes_dir().is_dir());
            check!("sysroots dir", ws.sysroots_dir().is_dir());
            check!("sources cache dir", ws.sources_dir().is_dir());

            let sandboxes = ws.list_sandboxes().unwrap_or_default();
            check!("at least one sandbox", !sandboxes.is_empty());
            if let Some(sb_dir) = sandboxes.first() {
                let prepared = sb_dir.join(".prepared").exists();
                check!(&format!("sandbox {} prepared", sb_dir.file_name().unwrap_or("?")), prepared);
            }

            let sysroots = sysroot::list(&ws).unwrap_or_default();
            check!(&format!("{} sysroot(s) available", sysroots.len()), !sysroots.is_empty());

            let boards = board::list(&boards_root).unwrap_or_default();
            check!(&format!("{} board(s) found", boards.len()), !boards.is_empty());

            println!("\n{ok} ok, {fail} issues");
            if fail > 0 {
                println!("Run 'crossdev-stages sandbox setup' and 'sandbox prepare' to fix.");
            }
        }

        // ── Status ──────────────────────────────────────────────────────────
        Commands::Status { tsv } => {
            let tty = !tsv;

            let sandboxes = sandbox::list(&ws)?;
            let sysroots = sysroot::list(&ws)?;
            let boards = board::list(&boards_root)?;
            let builds = ws.list_builds()?;

            if tty {
                println!("Sandboxes ({}):", sandboxes.len());
                for s in &sandboxes {
                    let state = if s.prepared { "prepared" } else { "unpacked" };
                    println!("  {:<20} {:<10} {}", s.name, s.arch, state);
                }
                println!("\nSysroots ({}):", sysroots.len());
                for s in &sysroots {
                    println!("  {:<25} {:<10} {}", s.name, s.arch, s.cflags);
                }
                println!("\nBoards ({}):", boards.len());
                for name in &boards {
                    if let Ok(b) = board::load(&boards_root, name) {
                        let tag = if b.testing { " [TESTING]" } else { "" };
                        println!("  {:<16} {:<10} sysroot={}{tag}", name, b.arch, b.sysroot);
                    }
                }
                println!("\nBuilds (latest {}/{}):", builds.len().min(5), builds.len());
                for dir in builds.iter().take(5) {
                    if let Some(b) = image::Build::open((*dir).clone()) {
                        let status = if b.dir.join(".packed").exists() { "packed" } else { "incomplete" };
                        let image = std::fs::read_to_string(b.dir.join(".image"))
                            .map(|s| format!(" ({})", s.trim()))
                            .unwrap_or_default();
                        println!("  {:<40} {}{image}", dir.file_name().unwrap_or("?"), status);
                    }
                }
            } else {
                for s in &sandboxes {
                    let state = if s.prepared { "prepared" } else { "unpacked" };
                    println!("sandbox\t{}\t{}\t{}", s.name, s.arch, state);
                }
                for s in &sysroots {
                    println!("sysroot\t{}\t{}\t{}", s.name, s.arch, s.cflags);
                }
                for name in &boards {
                    if let Ok(b) = board::load(&boards_root, name) {
                        println!("board\t{}\t{}\t{}\t{}", name, b.arch, b.sysroot, b.testing);
                    }
                }
                for dir in builds.iter().take(10) {
                    if let Some(b) = image::Build::open((*dir).clone()) {
                        let status = if b.dir.join(".packed").exists() { "packed" } else { "incomplete" };
                        let image = std::fs::read_to_string(b.dir.join(".image"))
                            .map(|s| s.trim().to_string())
                            .unwrap_or_else(|_| "-".into());
                        println!("build\t{}\t{}\t{}\t{}", dir.file_name().unwrap_or("?"), b.board, status, image);
                    }
                }
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
        opensbi_fw_type: None,
        opensbi_make_flags: None,
        u_boot_repo: None,
        u_boot_tag: None,
        u_boot_defconfig: None,
        u_boot_make_flags: None,
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
        compression: None,
        testing: false,
    }
}
