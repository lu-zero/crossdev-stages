use camino::Utf8Path;
use crate::{board, image, sandbox, workspace::Workspace};
use crate::error::Result;

pub fn run(ws: &Workspace, boards_root: &Utf8Path, tsv: bool) -> Result<()> {
    let tty = !tsv;

    let sandboxes = sandbox::list(ws)?;
    let boards = board::list(boards_root)?;
    let builds = ws.list_builds()?;

    if tty {
        println!("Sandboxes ({}):", sandboxes.len());
        for s in &sandboxes {
            let state = if s.prepared { "prepared" } else { "unpacked" };
            println!("  {:<20} {:<10} {}", s.name, s.arch, state);
        }
        println!("\nBoards ({}):", boards.len());
        for name in &boards {
            if let Ok(b) = board::load(boards_root, name) {
                let tag = if b.testing { " [TESTING]" } else { "" };
                println!("  {:<16} {:<10}{tag}", name, b.arch);
            }
        }
        println!("\nBuilds (latest {}/{}):", builds.len().min(5), builds.len());
        for dir in builds.iter().take(5) {
            if let Some(b) = image::Build::open((*dir).clone()) {
                let status = if b.dir.join(".packed").exists() { "packed" } else { "incomplete" };
                let image = std::fs::read_to_string(b.dir.join(".image"))
                    .map(|s| format!(" ({})", s.trim()))
                    .unwrap_or_default();
                let ts = dir.file_name().unwrap_or("?");
                println!("  {:<40} {}{image}", format!("{}/{}", b.board, ts), status);
            }
        }
    } else {
        for s in &sandboxes {
            let state = if s.prepared { "prepared" } else { "unpacked" };
            println!("sandbox\t{}\t{}\t{}", s.name, s.arch, state);
        }
        for name in &boards {
            if let Ok(b) = board::load(boards_root, name) {
                println!("board\t{}\t{}\t{}", name, b.arch, b.testing);
            }
        }
        for dir in builds.iter().take(10) {
            if let Some(b) = image::Build::open((*dir).clone()) {
                let status = if b.dir.join(".packed").exists() { "packed" } else { "incomplete" };
                let image = std::fs::read_to_string(b.dir.join(".image"))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|_| "-".into());
                println!(
                    "build\t{}/{}\t{}\t{}\t{}",
                    b.board,
                    dir.file_name().unwrap_or("?"),
                    b.board,
                    status,
                    image
                );
            }
        }
    }
    Ok(())
}
