use crate::error::Result;
use crate::{board, image, sandbox, workspace::Workspace};
use camino::Utf8Path;
use serde::Deserialize;
use std::collections::BTreeMap;

/// Build dir shown relative to builds/: `<board>/<timestamp>` for the
/// nested layout, just `<board>` for a legacy flat build.
fn build_id(ws: &Workspace, dir: &Utf8Path) -> String {
    dir.strip_prefix(ws.builds_dir())
        .map(|p| p.to_string())
        .unwrap_or_else(|_| dir.to_string())
}

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
    let stores = list_stores(ws);
    let default_specs = crate::cli::store::sandbox_default_specs(ws);

    if tty {
        println!("Sandboxes ({}):", sandboxes.len());
        for s in &sandboxes {
            let state = if s.prepared {
                "prepared"
            } else if s.bare_prepared {
                "bare"
            } else {
                "unpacked"
            };
            println!("  {:<20} {:<10} {}", s.name, s.arch, state);
        }
        println!("\nBoards ({}):", boards.len());
        for name in &boards {
            if let Ok(b) = board::load(boards_root, name) {
                let tag = if b.testing { " [TESTING]" } else { "" };
                let (_, hash) = crate::cflags::canonicalize(&b.effective_cflags());
                let keys = crate::cli::store::board_store_keys(&b, &default_specs);
                let store_state = board_store_state(&keys, &stores);
                println!(
                    "  {:<16} {:<10} {:<16} {}{tag}",
                    name, b.arch, hash, store_state,
                );
            }
        }
        if !stores.is_empty() {
            println!("\nStore ({}):", stores.len());
            for s in &stores {
                let state = if s.complete { "complete" } else { "partial" };
                println!("  {:<28} {:<30} {state}", s.chost, s.key);
            }
        }
        println!(
            "\nBuilds (latest {}/{}):",
            builds.len().min(5),
            builds.len()
        );
        for dir in builds.iter().take(5) {
            if let Some(b) = image::Build::open((*dir).clone()) {
                let status = if b.dir.join(".packed").exists() {
                    "packed"
                } else {
                    "incomplete"
                };
                let image = std::fs::read_to_string(b.dir.join(".image"))
                    .map(|s| format!(" ({})", s.trim()))
                    .unwrap_or_default();
                println!("  {:<40} {}{image}", build_id(ws, dir), status);
                if let Some(lock) = read_lock(&b.dir) {
                    print_sources_tty(&lock);
                }
            }
        }
    } else {
        for s in &sandboxes {
            let state = if s.prepared {
                "prepared"
            } else if s.bare_prepared {
                "bare"
            } else {
                "unpacked"
            };
            println!("sandbox\t{}\t{}\t{}", s.name, s.arch, state);
        }
        for name in &boards {
            if let Ok(b) = board::load(boards_root, name) {
                let (_, hash) = crate::cflags::canonicalize(&b.effective_cflags());
                let keys = crate::cli::store::board_store_keys(&b, &default_specs);
                let store_state = board_store_state(&keys, &stores);
                println!(
                    "board\t{}\t{}\t{}\t{}\t{}",
                    name, b.arch, b.testing, hash, store_state,
                );
            }
        }
        for s in &stores {
            let state = if s.complete { "complete" } else { "partial" };
            println!("store\t{}\t{}\t{state}", s.chost, s.key);
        }
        for dir in builds.iter().take(10) {
            if let Some(b) = image::Build::open((*dir).clone()) {
                let status = if b.dir.join(".packed").exists() {
                    "packed"
                } else {
                    "incomplete"
                };
                let image = std::fs::read_to_string(b.dir.join(".image"))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|_| "-".into());
                let id = build_id(ws, dir);
                println!("build\t{id}\t{}\t{status}\t{image}", b.board);
                if let Some(lock) = read_lock(&b.dir) {
                    print_sources_tsv(&id, &lock);
                }
            }
        }
    }
    Ok(())
}

/// State of the store entry a board resolves to.  `keys` are the exact
/// candidate store keys from `cli::store::board_store_keys` (same
/// `workspace::store_key` derivation the builders use): "ready" if any
/// is complete, "partial" if one exists but is incomplete, "missing" if
/// none, "unknown" when the gcc spec is unresolvable (no sandbox and no
/// BOARD_GCC_VERSION).
fn board_store_state(keys: &[camino::Utf8PathBuf], stores: &[StoreEntry]) -> &'static str {
    if keys.is_empty() {
        return "unknown";
    }
    let matching: Vec<&StoreEntry> = stores
        .iter()
        .filter(|s| keys.contains(&camino::Utf8PathBuf::from(&s.chost).join(&s.key)))
        .collect();
    if matching.iter().any(|s| s.complete) {
        "ready"
    } else if !matching.is_empty() {
        "partial"
    } else {
        "missing"
    }
}

struct StoreEntry {
    chost: String,
    /// Store key leaf: `<cflags-hash>-gcc<spec>` (opaque dir name).
    key: String,
    complete: bool,
}

/// Walk `store/<chost>/<cflags-hash>-gcc<spec>/` for every present prefix;
/// flag whether each carries a `.complete` marker.  Phase 3 makes drift
/// impossible by construction (each (chost, cflags-hash, gcc-spec) lives
/// in its own dir), so this simply reports what's available.
fn list_stores(ws: &Workspace) -> Vec<StoreEntry> {
    let root = ws.store_dir();
    let Ok(chost_iter) = std::fs::read_dir(&root) else {
        return vec![];
    };
    let mut entries = Vec::new();
    for chost_entry in chost_iter.flatten() {
        let Some(chost) = chost_entry.file_name().to_str().map(String::from) else {
            continue;
        };
        let Ok(key_iter) = std::fs::read_dir(chost_entry.path()) else {
            continue;
        };
        for key_entry in key_iter.flatten() {
            let Some(key) = key_entry.file_name().to_str().map(String::from) else {
                continue;
            };
            let complete = key_entry.path().join(".complete").exists();
            entries.push(StoreEntry {
                chost: chost.clone(),
                key,
                complete,
            });
        }
    }
    entries.sort_by(|a, b| {
        (a.chost.as_str(), a.key.as_str()).cmp(&(b.chost.as_str(), b.key.as_str()))
    });
    entries
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
