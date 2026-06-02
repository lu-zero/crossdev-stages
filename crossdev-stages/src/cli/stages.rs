use crate::cli::StagesCmd;
use crate::error::Result;
use crate::{stage, workspace::Workspace};

pub async fn run(ws: &Workspace, cmd: StagesCmd, mirror: Option<&str>) -> Result<()> {
    match cmd {
        StagesCmd::List { arch } => {
            let items = stage::list(&ws.stages_dir(), &arch, mirror).await?;
            for item in items {
                println!("{item}");
            }
        }
        StagesCmd::Fetch { arch } => {
            let path = stage::fetch(&ws.stages_dir(), &arch, mirror).await?;
            println!("{path}");
        }
    }
    Ok(())
}
