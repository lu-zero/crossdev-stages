use crate::cli::util::{default_board_config, ensure_crossdev};
use crate::cli::SandboxCmd;
use crate::error::Result;
use crate::{sandbox, stage, workspace::Workspace};

pub async fn run(
    ws: &Workspace,
    cmd: SandboxCmd,
    boards_root: &camino::Utf8Path,
    defaults_root: &camino::Utf8Path,
    mirror: Option<&str>,
) -> Result<()> {
    match cmd {
        SandboxCmd::List => {
            for s in sandbox::list(ws)? {
                let state = if s.prepared {
                    "prepared"
                } else if s.bare_prepared {
                    "bare"
                } else {
                    "unpacked"
                };
                println!("{:<20} arch={} state={}", s.name, s.arch, state);
            }
        }
        SandboxCmd::Setup { arch, name } => {
            let source_stage = stage::fetch(&ws.stages_dir(), &arch, mirror).await?;
            let name = name.unwrap_or_else(|| {
                format!("{arch}-{}", chrono::Utc::now().format("%Y%m%dT%H%M%SZ"))
            });
            sandbox::Sandbox::create(ws, &name, &arch, &source_stage)?;
            println!("Sandbox '{name}' created.");
        }
        SandboxCmd::Prepare { name, bare } => {
            let dir = ws.resolve_sandbox(name.as_deref())?;
            let sb = sandbox::Sandbox::open(dir)?;
            sb.prepare(mirror, defaults_root, bare)?;
        }
        SandboxCmd::Crossdev {
            arch,
            board,
            name,
            gcc_version,
        } => {
            let board_cfg = if let Some(b) = &board {
                crate::board::load(boards_root, b)?
            } else {
                default_board_config(&arch)
            };
            ensure_crossdev(
                ws,
                name.as_deref(),
                &arch,
                &board_cfg,
                defaults_root,
                mirror,
                gcc_version.as_deref(),
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
