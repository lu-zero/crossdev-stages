use camino::Utf8PathBuf;
use clap::builder::styling::{AnsiColor, Styles};
use clap::{Parser, Subcommand};

pub mod board;
pub mod image;
pub mod maint;
pub mod sandbox;
pub mod stages;
pub mod status;
pub mod target;
pub mod util;

// Saner default colored style.
const fn cli_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default().bold())
        .usage(AnsiColor::Green.on_default().bold())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Cyan.on_default())
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
pub struct Cli {
    /// Path to the project root (where boards/ lives). Defaults to the current directory.
    #[arg(long, global = true, default_value = ".")]
    pub project_dir: std::path::PathBuf,

    /// Gentoo mirror URL to use for downloads.
    #[arg(long, global = true)]
    pub mirror: Option<String>,

    /// Binary package host URL (sets PORTAGE_BINHOST).
    #[arg(long, global = true)]
    pub binhost: Option<String>,

    /// Extra portage config overlay directory applied on top of embedded defaults.
    #[arg(long, global = true, value_name = "DIR")]
    pub portage_overlay: Option<Utf8PathBuf>,

    /// Show what would be done without executing.
    #[arg(long, global = true)]
    pub dry_run: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Manage host build sandboxes.
    #[command(subcommand)]
    Sandbox(SandboxCmd),

    /// Manage cross-compiled target stages.
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

    /// Manage and inspect boards.
    #[command(subcommand)]
    Board(BoardCmd),

    /// Build board images.
    #[command(subcommand)]
    Image(ImageCmd),

    /// List or download Gentoo stage3 tarballs.
    #[command(subcommand)]
    Stages(StagesCmd),

    /// Maintenance: cleanup, logs, diagnostics.
    #[command(subcommand)]
    Maint(MaintCmd),

    /// Show overview of sandboxes, targets, builds, and boards.
    Status {
        /// Machine-readable TAB-separated output.
        #[arg(long)]
        tsv: bool,
    },
}

// ── Sandbox subcommands ──────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum SandboxCmd {
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
pub enum TargetCmd {
    /// Create a target stage from a stage3 tarball (downloaded or local).
    Setup {
        /// Target name (default: arch-<timestamp>).
        #[arg(long)]
        name: Option<String>,
        /// Use a local tarball instead of downloading (implies --arch from the file if not set).
        #[arg(long)]
        from: Option<Utf8PathBuf>,
    },
    /// List all target stages.
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
    /// Pack the target stage as a stage3-compatible tarball.
    Export {
        /// Output path for the tarball (default: stage3-<arch>-<name>.tar.xz in current dir).
        #[arg(long, short)]
        output: Option<Utf8PathBuf>,
        /// Compression: xz (default), gz, none.
        #[arg(long, default_value = "xz")]
        compression: String,
    },
}

// ── Board subcommands ────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum BoardCmd {
    /// List available boards.
    List,
    /// Show resolved configuration for a board.
    Info {
        /// Board name.
        board: String,
    },
}

// ── Image subcommands ────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum ImageCmd {
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
    /// Export the final image file from a build.
    Export {
        /// Board name.
        #[arg(long)]
        board: String,
        /// Output directory (default: current directory).
        #[arg(long, short)]
        output: Option<Utf8PathBuf>,
        /// Export all build artifacts, not just the final image.
        #[arg(long)]
        all: bool,
    },
}

// ── Stages subcommands ───────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum StagesCmd {
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

// ── Maint subcommands ────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum MaintCmd {
    /// Clean up stale builds and old stage3 tarballs.
    Cleanup {
        /// Remove everything (all builds, stages).
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
    /// Check environment for common issues.
    Doctor,
}
