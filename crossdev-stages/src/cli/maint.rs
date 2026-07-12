use crate::cli::{CleanArgs, MaintCmd};
use crate::error::Result;
use crate::{board, container, image, workspace::Workspace};
use camino::{Utf8Path, Utf8PathBuf};

pub fn run(
    ws: &Workspace,
    cmd: MaintCmd,
    boards_root: &camino::Utf8Path,
    dry_run: bool,
) -> Result<()> {
    match cmd {
        MaintCmd::Clean(args) => clean(ws, args, dry_run),
        MaintCmd::Logs { board, step } => logs(ws, &board, step.as_deref()),
        MaintCmd::Doctor => doctor(ws, boards_root),
        MaintCmd::Recover => recover(ws, dry_run),
    }
}

fn clean(ws: &Workspace, args: CleanArgs, dry_run: bool) -> Result<()> {
    let all = args.all;
    let categories: [(&str, &str, bool, Utf8PathBuf); 6] = [
        (
            "sandbox",
            "sandboxes",
            args.sandboxes || all,
            ws.sandboxes_dir(),
        ),
        ("target", "targets", args.targets || all, ws.targets_dir()),
        ("build", "builds", args.builds || all, ws.builds_dir()),
        ("source", "sources", args.sources || all, ws.sources_dir()),
        ("stage", "stages", args.stages || all, ws.stages_dir()),
        ("log", "logs", args.logs || all, ws.logs_dir()),
    ];

    if categories.iter().all(|(_, _, selected, _)| !selected) {
        return gc(ws, dry_run);
    }
    for (singular, plural, selected, dir) in categories {
        if selected {
            clean_category(ws, singular, plural, &dir, dry_run)?;
        }
    }
    Ok(())
}

/// Default `maint clean`: drop incomplete builds and stage3 tarballs
/// older than the newest per arch.
fn gc(ws: &Workspace, dry_run: bool) -> Result<()> {
    let mut total = 0usize;

    // 1. Incomplete builds
    for dir in &ws.list_builds()? {
        if !dir.join(".packed").exists() {
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
            for (path, _) in files.get(1..).unwrap_or(&[]) {
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

fn clean_category(
    ws: &Workspace,
    singular: &str,
    plural: &str,
    dir: &Utf8Path,
    dry_run: bool,
) -> Result<()> {
    let mut count = 0usize;
    let mut freed = 0u64;

    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = match Utf8PathBuf::try_from(entry.path()) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let name = path.file_name().unwrap_or("?");
            let size = du(&path);
            if dry_run {
                container::recover_mounts_for_removal(&path, true)?;
                println!("Would remove {singular}: {name} ({})", human(size));
            } else {
                remove_entry(&path, ws.base())?;
                println!("Removed {singular}: {name} ({})", human(size));
            }
            count += 1;
            freed += size;
        }
    }

    if dry_run {
        println!("would remove {count} {plural} (~{})", human(freed));
    } else {
        println!("removed {count} {plural} (freed ~{})", human(freed));
    }
    Ok(())
}

/// Remove one top-level entry. Directories may contain files owned by
/// subordinate uids (portage etc.), so remove them via `rm -rf` inside a
/// container with the full subuid/gid maps.
fn remove_entry(path: &Utf8Path, cache_base: &Utf8Path) -> Result<()> {
    container::recover_mounts_for_removal(path, false)?;
    if std::fs::symlink_metadata(path.as_std_path())?.is_dir() {
        container::destroy_dir(path, cache_base)
    } else {
        Ok(std::fs::remove_file(path)?)
    }
}

fn recover(ws: &Workspace, dry_run: bool) -> Result<()> {
    let count = container::recover_sandbox_mounts(ws, dry_run)?;
    if dry_run {
        if count == 0 {
            println!("No stale sandbox mounts found.");
        } else {
            println!("{count} stale mount(s) would be unmounted.");
        }
    } else if count == 0 {
        println!("No stale sandbox mounts found.");
    } else {
        println!("Recovered {count} stale mount(s).");
    }
    Ok(())
}

/// Approximate disk usage (apparent size); unreadable entries count as 0.
fn du(path: &Utf8Path) -> u64 {
    fn walk(p: &std::path::Path) -> u64 {
        let Ok(meta) = std::fs::symlink_metadata(p) else {
            return 0;
        };
        if meta.is_dir() {
            std::fs::read_dir(p)
                .map(|rd| rd.flatten().map(|e| walk(&e.path())).sum())
                .unwrap_or(0)
        } else {
            meta.len()
        }
    }
    walk(path.as_std_path())
}

fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
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

    let stale_mounts = container::find_stale_sandbox_mounts(ws).unwrap_or_default();
    check!("no stale sandbox mounts", stale_mounts.is_empty());
    for mount in &stale_mounts {
        println!("      stale: {} ({})", mount.path, mount.sandbox);
    }

    println!("\n{ok} ok, {fail} issues");
    if fail > 0 {
        if !stale_mounts.is_empty() {
            println!("Run 'crossdev-stages maint recover' to unmount stale sandbox mounts.");
        }
        if sandboxes.is_empty() || sandboxes.first().is_some_and(|d| !d.join(".prepared").exists())
        {
            println!("Run 'crossdev-stages sandbox setup' and 'sandbox prepare' to fix.");
        }
    }
    Ok(())
}
