mod board;
mod bootloader;
mod cli;
mod container;
mod error;
mod image;
mod package_list;
mod portage;
mod sandbox;
mod source_cache;
mod stage;
mod target;
mod workspace;

use camino::Utf8PathBuf;
use clap::Parser;

use cli::{Cli, Commands};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter("crossdev_stages=info")
        .init();

    let ws = crate::workspace::Workspace::open()?;
    ws.ensure_dirs()?;

    let project_dir = {
        let p = std::fs::canonicalize(&cli.project_dir)
            .unwrap_or_else(|_| cli.project_dir.clone());
        Utf8PathBuf::try_from(p).expect("project path is not UTF-8")
    };
    let boards_root = project_dir.join("boards");
    let defaults_root = project_dir.join("defaults");
    let mirror = cli.mirror.as_deref();
    let dry_run = cli.dry_run;

    match cli.command {
        Commands::Stages(cmd) => {
            cli::stages::run(&ws, cmd, mirror).await?;
        }
        Commands::Sandbox(cmd) => {
            cli::sandbox::run(&ws, cmd, &boards_root, &defaults_root, mirror).await?;
        }
        Commands::Target { arch, sandbox, target, command } => {
            cli::target::run(&ws, arch, sandbox, target, command, &defaults_root, mirror).await?;
        }
        Commands::Board(cmd) => {
            cli::board::run(&boards_root, cmd)?;
        }
        Commands::Image(cmd) => {
            cli::image::run(&ws, cmd, &boards_root, &defaults_root, mirror, dry_run).await?;
        }
        Commands::Maint(cmd) => {
            cli::maint::run(&ws, cmd, &boards_root, dry_run)?;
        }
        Commands::Status { tsv } => {
            cli::status::run(&ws, &boards_root, tsv)?;
        }
    }

    Ok(())
}
