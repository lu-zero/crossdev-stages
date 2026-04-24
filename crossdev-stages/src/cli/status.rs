use camino::Utf8Path;
use serde::Deserialize;
use std::collections::BTreeMap;

use crate::{board, image, sandbox, workspace::Workspace};
use crate::error::Result;

/// Subset of `build.lock.toml` that status reads; wider fields ignored.
#[derive(Debug, Deserialize)]
struct LockSummary {
    sources: Option<BTreeMap<String, LockSource>>,
}

#[derive(Debug, Deserialize)]
struct LockSource {
    tag: String,
    commit: String,
    #[serde(default)]
    kind: Option<String>,
}

fn read_lock(build_dir: &Utf8Path) -> Option<LockSummary> {
    let path = build_dir.join("build.lock.toml");
    let body = std::fs::read_to_string(path).ok()?;
    toml::from_str(&body).ok()
}

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
                if let Some(lock) = read_lock(&b.dir) {
                    print_sources_tty(&lock);
                }
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
                let build_id = format!("{}/{}", b.board, dir.file_name().unwrap_or("?"));
                println!("build\t{build_id}\t{}\t{status}\t{image}", b.board);
                if let Some(lock) = read_lock(&b.dir) {
                    print_sources_tsv(&build_id, &lock);
                }
            }
        }
    }
    Ok(())
}

fn print_sources_tty(lock: &LockSummary) {
    let Some(sources) = &lock.sources else { return };
    let mut parts = Vec::new();
    for (name, src) in sources {
        let short = src.commit.chars().take(8).collect::<String>();
        let unpinned = matches!(src.tag.as_str(), "master" | "main" | "trunk" | "HEAD");
        let marker = if unpinned { "*" } else { "" };
        parts.push(format!("{name} {}@{short}{marker}", src.tag));
    }
    println!("      sources: {}", parts.join(" | "));
}

fn print_sources_tsv(build_id: &str, lock: &LockSummary) {
    let Some(sources) = &lock.sources else { return };
    for (name, src) in sources {
        let kind = src.kind.as_deref().unwrap_or("unknown");
        println!(
            "source\t{build_id}\t{name}\t{}\t{}\t{kind}",
            src.tag, src.commit
        );
    }
}

