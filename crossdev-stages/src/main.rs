mod board;
mod container;
mod error;
mod image;
mod portage;
mod sandbox;
mod stage;
mod target;
mod workspace;

use clap::{Parser, Subcommand};

use error::Result;
use workspace::Workspace;

// ── Top-level CLI ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "crossdev-stages", about = "Gentoo-based cross-compilation stage builder")]
struct Cli {
    /// Path to the project root (where boards/ lives). Defaults to the current directory.
    #[arg(long, global = true, default_value = ".")]
    project_dir: std::path::PathBuf,

    /// Gentoo mirror URL to use for downloads.
    #[arg(long, global = true)]
    mirror: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage host build sandboxes.
    #[command(subcommand)]
    Sandbox(SandboxCmd),

    /// Manage target sysroots.
    #[command(subcommand)]
    Target(TargetCmd),

    /// Build board images.
    #[command(subcommand)]
    Image(ImageCmd),

    /// List or download Gentoo stage3 tarballs.
    #[command(subcommand)]
    Stages(StagesCmd),
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
    Destroy {
        name: String,
    },
}

// ── Target subcommands ───────────────────────────────────────────────────────

#[derive(Subcommand)]
enum TargetCmd {
    /// Download a stage3 and create a target sysroot.
    Setup {
        #[arg(long, default_value = "riscv64")]
        arch: String,
        #[arg(long)]
        name: Option<String>,
    },
    /// List all target sysroots.
    List,
    /// Bootstrap the target: cross-emerge baselayout → @system → portage.
    Stage1 {
        #[arg(long)]
        sandbox: Option<String>,
        #[arg(long)]
        target: Option<String>,
    },
    /// Update the target (@world rebuild).
    Update {
        #[arg(long)]
        sandbox: Option<String>,
        #[arg(long)]
        target: Option<String>,
    },
    /// Cross-emerge packages into the target.
    Install {
        #[arg(long)]
        sandbox: Option<String>,
        #[arg(long)]
        target: Option<String>,
        packages: Vec<String>,
    },
    /// Update ldconfig cache in the target.
    Ldconfig {
        #[arg(long)]
        sandbox: Option<String>,
        #[arg(long)]
        target: Option<String>,
    },
    /// Remove a target.
    Destroy {
        name: String,
    },
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
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();
    let ws = Workspace::open()?;
    ws.ensure_dirs()?;

    let boards_root = cli.project_dir.join("boards");
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
                format!(
                    "{arch}-{}",
                    chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
                )
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
            let dir = ws.resolve_sandbox(name.as_deref())?;
            let sb = sandbox::Sandbox::open(dir)?;
            // Load board config for CFLAGS and workarounds, or use a minimal default.
            let board_cfg = if let Some(b) = &board {
                board::load(&boards_root, b)?
            } else {
                default_board_config(&arch)
            };
            sb.setup_crossdev(&arch, &board_cfg)?;
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
        Commands::Target(TargetCmd::List) => {
            for t in target::list(&ws)? {
                let s1 = if t.stage1 { "stage1" } else { "unpacked" };
                let upd = t.updated.as_deref().unwrap_or("-");
                println!("{:<20} arch={} state={} updated={}", t.name, t.arch, s1, upd);
            }
        }
        Commands::Target(TargetCmd::Setup { arch, name }) => {
            let stage_file = stage::fetch(&ws.stages_dir(), &arch, mirror).await?;
            let name = name.unwrap_or_else(|| {
                format!("{arch}-{}", chrono::Utc::now().format("%Y%m%dT%H%M%SZ"))
            });
            target::Target::create(&ws, &name, &arch, &stage_file)?;
            println!("Target '{name}' created.");
        }
        Commands::Target(TargetCmd::Stage1 { sandbox, target }) => {
            let sd = ws.resolve_sandbox(sandbox.as_deref())?;
            let td = ws.resolve_target(target.as_deref())?;
            let sb = sandbox::Sandbox::open(sd)?;
            let tgt = target::Target::open(td)?;
            tgt.build_stage1(&sb)?;
        }
        Commands::Target(TargetCmd::Update { sandbox, target }) => {
            let sd = ws.resolve_sandbox(sandbox.as_deref())?;
            let td = ws.resolve_target(target.as_deref())?;
            let sb = sandbox::Sandbox::open(sd)?;
            let tgt = target::Target::open(td)?;
            tgt.update(&sb)?;
        }
        Commands::Target(TargetCmd::Install { sandbox, target, packages }) => {
            let sd = ws.resolve_sandbox(sandbox.as_deref())?;
            let td = ws.resolve_target(target.as_deref())?;
            let sb = sandbox::Sandbox::open(sd)?;
            let tgt = target::Target::open(td)?;
            let pkgs: Vec<&str> = packages.iter().map(String::as_str).collect();
            tgt.install(&sb, &pkgs)?;
        }
        Commands::Target(TargetCmd::Ldconfig { sandbox, target }) => {
            let sd = ws.resolve_sandbox(sandbox.as_deref())?;
            let td = ws.resolve_target(target.as_deref())?;
            let sb = sandbox::Sandbox::open(sd)?;
            let tgt = target::Target::open(td)?;
            tgt.update_ldconfig(&sb)?;
        }
        Commands::Target(TargetCmd::Destroy { name }) => {
            let dir = ws.target(&name);
            if dir.is_dir() {
                std::fs::remove_dir_all(&dir)?;
                println!("Removed target '{name}'.");
            } else {
                eprintln!("Target '{name}' does not exist.");
            }
        }

        // ── Image ────────────────────────────────────────────────────────────
        Commands::Image(ImageCmd::ListBoards) => {
            for b in board::list(&boards_root)? {
                println!("{b}");
            }
        }
        Commands::Image(ImageCmd::Build { board: board_name, sandbox, target, steps }) => {
            let sd = ws.resolve_sandbox(sandbox.as_deref())?;
            let td = ws.resolve_target(target.as_deref())?;
            let sb = sandbox::Sandbox::open(sd)?;
            let tgt = target::Target::open(td)?;
            let board_cfg = board::load(&boards_root, &board_name)?;
            let steps_opt = if steps.is_empty() { None } else { Some(steps.as_slice()) };
            image::build(&ws, &sb, &tgt, &board_cfg, &boards_root, steps_opt)?;
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
    }

    Ok(())
}

/// Build a minimal `BoardConfig` when no board is specified for crossdev setup.
fn default_board_config(arch: &str) -> board::BoardConfig {
    board::BoardConfig {
        name: arch.to_string(),
        arch: arch.to_string(),
        cflags: None,
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
    }
}
