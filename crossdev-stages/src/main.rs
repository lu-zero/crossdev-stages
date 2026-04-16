mod cli;

use camino::Utf8PathBuf;
use clap::Parser;

use cli::{Cli, Commands};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter("crossdev_stages=info")
        .init();

    let ws = crossdev_stages::workspace::Workspace::open()?;
    ws.ensure_dirs()?;

    let boards_root = {
        let p = std::fs::canonicalize(cli.project_dir.join("boards"))
            .unwrap_or_else(|_| cli.project_dir.join("boards"));
        Utf8PathBuf::try_from(p).expect("boards path is not UTF-8")
    };
    let mirror = cli.mirror.as_deref();
    let portage_overlay = cli.portage_overlay.as_deref();
    let dry_run = cli.dry_run;

    match cli.command {
        Commands::Stages(cmd) => {
            cli::stages::run(&ws, cmd, mirror).await?;
        }
        Commands::Sandbox(cmd) => {
            cli::sandbox::run(&ws, cmd, &boards_root, mirror, portage_overlay).await?;
        }
        Commands::Target { arch, sandbox, target, command } => {
            cli::target::run(&ws, arch, sandbox, target, command, mirror, portage_overlay).await?;
        }
        Commands::Board(cmd) => {
            cli::board::run(&boards_root, cmd)?;
        }
        Commands::Image(cmd) => {
            cli::image::run(&ws, cmd, &boards_root, mirror, portage_overlay, dry_run).await?;
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
