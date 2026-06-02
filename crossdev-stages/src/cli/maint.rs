use crate::cli::MaintCmd;
use crate::error::Result;
use crate::{board, container, image, workspace::Workspace};
use camino::Utf8PathBuf;

pub fn run(
    ws: &Workspace,
    cmd: MaintCmd,
    boards_root: &camino::Utf8Path,
    dry_run: bool,
) -> Result<()> {
    match cmd {
        MaintCmd::Cleanup { all } => cleanup(ws, all, dry_run),
        MaintCmd::Logs { board, step } => logs(ws, &board, step.as_deref()),
        MaintCmd::Doctor => doctor(ws, boards_root),
    }
}

fn cleanup(ws: &Workspace, all: bool, dry_run: bool) -> Result<()> {
    let mut total = 0usize;

    // 1. Incomplete builds (always); all builds if --all
    for dir in &ws.list_builds()? {
        if all || !dir.join(".packed").exists() {
            let name = dir.file_name().unwrap_or("?");
            if dry_run {
                println!("Would remove build: {name}");
            } else {
                container::destroy_dir(dir, ws.base())?;
                println!("Removed build: {name}");
            }
            total += 1;
        }
    }

    // 2. Old stage3 tarballs: keep the newest per arch, remove the rest
    let stages_dir = ws.stages_dir();
    if stages_dir.is_dir() {
        let mut by_arch: std::collections::HashMap<
            String,
            Vec<(Utf8PathBuf, std::time::SystemTime)>,
        > = std::collections::HashMap::new();

        for entry in std::fs::read_dir(&stages_dir)? {
            let entry = entry?;
            let path = match Utf8PathBuf::try_from(entry.path()) {
                Ok(p) if p.is_file() => p,
                _ => continue,
            };
            let fname = path.file_name().unwrap_or("").to_string();
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
            files.sort_by_key(|b| std::cmp::Reverse(b.1)); // newest first
            let to_remove = if all {
                &files[..]
            } else {
                files.get(1..).unwrap_or(&[])
            };
            for (path, _) in to_remove {
                let name = path.file_name().unwrap_or("?");
                if dry_run {
                    println!("Would remove stage: {name}");
                } else {
                    std::fs::remove_file(path)?;
                    println!("Removed stage: {name}");
                }
                total += 1;
            }
        }
    }

    if dry_run {
        println!("{total} item(s) would be removed.");
    } else {
        println!("{total} item(s) cleaned up.");
    }
    Ok(())
}

fn logs(ws: &Workspace, board_name: &str, step: Option<&str>) -> Result<()> {
    let builds = ws.list_builds()?;
    let build = builds
        .iter()
        .filter_map(|dir| image::Build::open(dir.clone()))
        .find(|b| b.board == board_name)
        .ok_or_else(|| {
            crate::error::Error::BoardNotFound(format!("no builds for '{board_name}'"))
        })?;

    println!("Build: {}", build.dir);
    println!("Board: {}", build.board);

    for s in &[
        "deps",
        "sources",
        "bootloader",
        "kernel",
        "assembled",
        "packed",
    ] {
        let marker = build.dir.join(format!(".{s}"));
        if marker.exists() {
            let ts = std::fs::read_to_string(&marker).unwrap_or_default();
            let label = if step == Some(s) { " <--" } else { "" };
            println!("  {s}: {}{label}", ts.trim());
        }
    }

    if let Some(step_name) = step {
        let log_dir = ws.logs_dir();
        let pattern = format!("{board_name}-");
        let mut found = false;
        if log_dir.is_dir() {
            for entry in std::fs::read_dir(&log_dir)? {
                let entry = entry?;
                let name = entry.file_name().into_string().unwrap_or_default();
                if name.contains(&pattern) && name.contains(step_name) {
                    println!("\n--- {name} ---");
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
    Ok(())
}

fn doctor(ws: &Workspace, boards_root: &camino::Utf8Path) -> Result<()> {
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
    check!("sources cache dir", ws.sources_dir().is_dir());

    let sandboxes = ws.list_sandboxes().unwrap_or_default();
    check!("at least one sandbox", !sandboxes.is_empty());
    if let Some(sb_dir) = sandboxes.first() {
        let prepared = sb_dir.join(".prepared").exists();
        check!(
            &format!("sandbox {} prepared", sb_dir.file_name().unwrap_or("?")),
            prepared
        );
    }

    let boards = board::list(boards_root).unwrap_or_default();
    check!(
        &format!("{} board(s) found", boards.len()),
        !boards.is_empty()
    );

    println!("\n{ok} ok, {fail} issues");
    if fail > 0 {
        println!("Run 'crossdev-stages sandbox setup' and 'sandbox prepare' to fix.");
    }
    Ok(())
}
