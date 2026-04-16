use crossdev_stages::{sandbox, stage, workspace::Workspace};
use crossdev_stages::error::Result;
use crate::cli::SandboxCmd;
use crate::cli::util::{default_board_config, ensure_crossdev};

pub async fn run(
    ws: &Workspace,
    cmd: SandboxCmd,
    boards_root: &camino::Utf8Path,
    mirror: Option<&str>,
    portage_overlay: Option<&camino::Utf8Path>,
) -> Result<()> {
    match cmd {
        SandboxCmd::List => {
            for s in sandbox::list(ws)? {
                let state = if s.prepared { "prepared" } else { "unpacked" };
                println!("{:<20} arch={} state={}", s.name, s.arch, state);
            }
        }
        SandboxCmd::Setup { arch, name } => {
            let stage_file = stage::fetch(&ws.stages_dir(), &arch, mirror).await?;
            let name = name.unwrap_or_else(|| {
                format!("{arch}-{}", chrono::Utc::now().format("%Y%m%dT%H%M%SZ"))
            });
            sandbox::Sandbox::create(ws, &name, &arch, &stage_file)?;
            println!("Sandbox '{name}' created.");
        }
        SandboxCmd::Prepare { name } => {
            let dir = ws.resolve_sandbox(name.as_deref())?;
            let sb = sandbox::Sandbox::open(dir)?;
            sb.prepare(mirror, portage_overlay)?;
        }
        SandboxCmd::Crossdev { arch, board, name } => {
            let board_cfg = if let Some(b) = &board {
                crossdev_stages::board::load(boards_root, b)?
            } else {
                default_board_config(&arch)
            };
            ensure_crossdev(
                ws,
                name.as_deref(),
                &arch,
                &board_cfg,
                mirror,
                portage_overlay,
            )
            .await?;
        }
        SandboxCmd::Enter { name } => {
            let dir = ws.resolve_sandbox(name.as_deref())?;
            let sb = sandbox::Sandbox::open(dir)?;
            sb.runner().shell()?;
        }
        SandboxCmd::Run { name, cmd } => {
            let dir = ws.resolve_sandbox(name.as_deref())?;
            let sb = sandbox::Sandbox::open(dir)?;
            sb.runner().run(&cmd.join(" "))?;
        }
        SandboxCmd::Destroy { name } => {
            sandbox::destroy(ws, &name)?;
        }
    }
    Ok(())
}
