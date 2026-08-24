use std::collections::BTreeSet;

use camino::{Utf8Path, Utf8PathBuf};

use crate::board::BoardConfig;
use crate::cli::StoreCmd;
use crate::error::{Error, Result};
use crate::workspace::{store_key, Workspace};

pub fn run(ws: &Workspace, boards_root: &Utf8Path, cmd: StoreCmd) -> Result<()> {
    match cmd {
        StoreCmd::List => list(ws),
        StoreCmd::Gc { force } => gc(ws, boards_root, force),
    }
}

fn list(ws: &Workspace) -> Result<()> {
    let entries = walk_store(ws);
    if entries.is_empty() {
        println!("Store is empty.");
        return Ok(());
    }
    println!("{:<32} {:<34} state", "chost", "key");
    for e in &entries {
        let state = if e.complete { "complete" } else { "partial" };
        println!("{:<32} {:<34} {state}", e.chost, e.key);
    }
    println!("\n{} entries.", entries.len());
    Ok(())
}

fn gc(ws: &Workspace, boards_root: &Utf8Path, force: bool) -> Result<()> {
    let entries = walk_store(ws);
    let binpkgs_entries = walk_binpkgs(ws);
    let live = live_set(ws, boards_root)?;
    ensure_live_nonempty(&live, boards_root)?;

    let unused = unused_entries(entries, &live.store);
    let unused_binpkgs = unused_entries(binpkgs_entries, &live.binpkgs);

    if unused.is_empty() && unused_binpkgs.is_empty() {
        println!("No unused store or binpkg entries.");
        return Ok(());
    }

    if !unused.is_empty() {
        println!(
            "{} unused store entr{}:",
            unused.len(),
            if unused.len() == 1 { "y" } else { "ies" }
        );
        for e in &unused {
            let state = if e.complete { "complete" } else { "partial" };
            let size = dir_size_human(&e.path);
            println!("  {:<32} {:<34} {state:<8} {size}", e.chost, e.key);
        }
    }
    if !unused_binpkgs.is_empty() {
        println!(
            "{} unused binpkg cache{}:",
            unused_binpkgs.len(),
            if unused_binpkgs.len() == 1 { "" } else { "s" },
        );
        for e in &unused_binpkgs {
            let size = dir_size_human(&e.path);
            println!("  {:<32} {:<34} {size}", e.chost, e.key);
        }
    }
    if !force {
        println!("\nRe-run with --force to delete.");
        return Ok(());
    }

    let mut removed = 0;
    let total = unused.len() + unused_binpkgs.len();
    for e in unused.iter().chain(unused_binpkgs.iter()) {
        match crate::container::destroy_dir(&e.path, ws.base()) {
            Ok(()) => {
                println!("Removed {}/{}", e.chost, e.key);
                removed += 1;
            }
            Err(err) => println!("Failed to remove {}/{}: {err}", e.chost, e.key),
        }
    }
    println!("\nRemoved {removed}/{total} entries.");
    Ok(())
}

struct StoreEntry {
    chost: String,
    /// Dir name under the chost level: `<cflags-hash>-gcc<spec>` for the
    /// store, bare `<cflags-hash>` for the binpkg cache (PKGDIR is shared
    /// across gcc specs).  Opaque either way.
    key: String,
    complete: bool,
    path: Utf8PathBuf,
}

impl StoreEntry {
    /// Path relative to the walked root; comparable to [`store_key`] output.
    fn rel_key(&self) -> Utf8PathBuf {
        Utf8PathBuf::from(&self.chost).join(&self.key)
    }
}

/// Pure classification: entries whose relative key is not in the live set.
fn unused_entries(entries: Vec<StoreEntry>, live: &BTreeSet<Utf8PathBuf>) -> Vec<StoreEntry> {
    entries
        .into_iter()
        .filter(|e| !live.contains(&e.rel_key()))
        .collect()
}

fn walk_store(ws: &Workspace) -> Vec<StoreEntry> {
    walk_two_level(&ws.store_dir(), |path| path.join(".complete").exists())
}

fn walk_binpkgs(ws: &Workspace) -> Vec<StoreEntry> {
    walk_two_level(&ws.binpkgs_dir(), |_| false)
}

fn walk_two_level(root: &Utf8Path, mark_complete: impl Fn(&Utf8Path) -> bool) -> Vec<StoreEntry> {
    let Ok(chost_iter) = std::fs::read_dir(root) else {
        return vec![];
    };
    let mut out = Vec::new();
    for chost_entry in chost_iter.flatten() {
        let Some(chost) = chost_entry.file_name().to_str().map(String::from) else {
            continue;
        };
        let chost_path = match Utf8PathBuf::try_from(chost_entry.path()) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let Ok(key_iter) = std::fs::read_dir(&chost_path) else {
            continue;
        };
        for key_entry in key_iter.flatten() {
            let Some(key) = key_entry.file_name().to_str().map(String::from) else {
                continue;
            };
            let path = match Utf8PathBuf::try_from(key_entry.path()) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let complete = mark_complete(&path);
            out.push(StoreEntry {
                chost: chost.clone(),
                key,
                complete,
                path,
            });
        }
    }
    out.sort_by(|a, b| (a.chost.as_str(), a.key.as_str()).cmp(&(b.chost.as_str(), b.key.as_str())));
    out
}

/// Default gcc specs known to this workspace: the highest installed slot
/// of every existing sandbox (the spec `setup_crossdev` keys the store
/// with when neither `--gcc-version` nor `BOARD_GCC_VERSION` is set).
/// Reads each sandbox's `.gcc_versions` cache; a sandbox without the
/// cache gets one `qlist` run.
pub fn sandbox_default_specs(ws: &Workspace) -> Vec<String> {
    let Ok(dirs) = ws.list_sandboxes() else {
        return vec![];
    };
    let mut specs: Vec<String> = dirs
        .into_iter()
        .filter_map(|d| crate::sandbox::Sandbox::open(d).ok())
        .filter_map(|s| s.default_gcc_spec().ok())
        .collect();
    specs.sort();
    specs.dedup();
    specs
}

/// Candidate store keys `board` may build into.  Mirrors
/// `Sandbox::gcc_spec_for` resolution: `BOARD_GCC_VERSION` verbatim when
/// set, otherwise one key per known sandbox default spec.  Empty only
/// when the board needs a default spec and no sandbox can provide one.
pub fn board_store_keys(board: &BoardConfig, default_specs: &[String]) -> Vec<Utf8PathBuf> {
    let chost = board.chost();
    let hash = crate::cflags::key_for_board(board);
    let specs: Vec<&str> = match &board.gcc_version {
        Some(s) => vec![s.as_str()],
        None => default_specs.iter().map(String::as_str).collect(),
    };
    specs
        .into_iter()
        .map(|s| store_key(&chost, &hash, s))
        .collect()
}

/// An empty live set means no boards were found (wrong --project-dir?):
/// the store is global (XDG cache) while the live set comes from ./boards,
/// so proceeding would classify every store entry as unused and, with
/// --force, wipe the entire store.
fn ensure_live_nonempty(live: &LiveSet, boards_root: &Utf8Path) -> Result<()> {
    if live.store.is_empty() {
        return Err(Error::CommandFailed {
            code: 1,
            reason: format!("no boards found under {boards_root}; refusing to gc"),
        });
    }
    Ok(())
}

/// Everything the project's boards can still reach: store keys
/// (`<chost>/<cflags-hash>-gcc<spec>`) and binpkg cache dirs
/// (`<chost>/<cflags-hash>`, gcc-spec independent).
struct LiveSet {
    store: BTreeSet<Utf8PathBuf>,
    binpkgs: BTreeSet<Utf8PathBuf>,
}

/// Store keys and binpkg dirs every known board (and the per-arch
/// default-CFLAGS flows: `target stage1 / update / install` without a
/// board context) would resolve to.  An entry NOT in this set is
/// unreachable from the project's boards and a candidate for GC.
///
/// Keys are derived through the same [`store_key`] fn the builders use;
/// deriving them any other way would classify live entries as garbage.
/// The gcc spec is resolved exactly like `Sandbox::gcc_spec_for`:
/// `BOARD_GCC_VERSION` verbatim, else the sandbox default.  Without any
/// sandbox the default spec is unknowable, so gc refuses rather than
/// guess.
fn live_set(ws: &Workspace, boards_root: &Utf8Path) -> Result<LiveSet> {
    let default_specs = sandbox_default_specs(ws);
    if default_specs.is_empty() {
        return Err(Error::CommandFailed {
            code: 1,
            reason: "cannot resolve the default gcc spec (no usable sandbox); \
                     run sandbox setup/prepare first"
                .into(),
        });
    }
    let mut live = LiveSet {
        store: BTreeSet::new(),
        binpkgs: BTreeSet::new(),
    };
    let mut arches = BTreeSet::new();
    for name in crate::board::list(boards_root)? {
        let Ok(b) = crate::board::load(boards_root, &name) else {
            continue;
        };
        live.store.extend(board_store_keys(&b, &default_specs));
        let hash = crate::cflags::key_for_board(&b);
        live.binpkgs.insert(Utf8PathBuf::from(b.chost()).join(hash));
        arches.insert(b.arch.clone());
    }
    // Default-cflags entries: what `target stage1 / update / install`
    // resolve to without a board context.  Keep them too.
    for arch in arches {
        let chost = crate::stage::chost_for_arch(&arch)
            .unwrap_or_else(|_| format!("{arch}-unknown-linux-gnu"));
        let (_, hash) = crate::cflags::canonicalize(crate::stage::default_cflags(&arch));
        for spec in &default_specs {
            live.store.insert(store_key(&chost, &hash, spec));
        }
        live.binpkgs.insert(Utf8PathBuf::from(chost).join(hash));
    }
    Ok(live)
}

fn dir_size_human(p: &Utf8Path) -> String {
    let bytes = walk_size(p);
    if bytes >= 1_073_741_824 {
        format!("{:.1}G", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1}M", bytes as f64 / 1_048_576.0)
    } else {
        format!("{}K", bytes / 1024)
    }
}

fn walk_size(p: &Utf8Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(p) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(p) else {
        return 0;
    };
    let mut total = 0;
    for e in entries.flatten() {
        if let Ok(child) = Utf8PathBuf::try_from(e.path()) {
            total += walk_size(&child);
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(chost: &str, key: &str) -> StoreEntry {
        StoreEntry {
            chost: chost.into(),
            key: key.into(),
            complete: true,
            path: Utf8PathBuf::from("/store").join(chost).join(key),
        }
    }

    #[test]
    fn live_entry_survives_stale_sibling_collected() {
        // The live set is built through the exact same store_key fn the
        // builders use, so a live board's dir is never classified unused
        // while a sibling keyed for a stale gcc spec or CFLAGS hash is.
        let chost = "riscv64-unknown-linux-gnu";
        let live: BTreeSet<Utf8PathBuf> = [store_key(chost, "0123456789abcdef", "15")]
            .into_iter()
            .collect();

        let entries = vec![
            entry(chost, "0123456789abcdef-gcc15"), // live
            entry(chost, "0123456789abcdef-gcc14"), // stale gcc sibling
            entry(chost, "fedcba9876543210-gcc15"), // stale cflags sibling
        ];
        let unused = unused_entries(entries, &live);
        let keys: Vec<_> = unused.iter().map(|e| e.rel_key()).collect();
        assert_eq!(
            keys,
            vec![
                Utf8PathBuf::from(chost).join("0123456789abcdef-gcc14"),
                Utf8PathBuf::from(chost).join("fedcba9876543210-gcc15"),
            ]
        );
    }

    #[test]
    fn binpkg_dirs_classify_without_gcc_spec() {
        // binpkgs/<chost>/<hash> is shared across gcc specs; live-ness is
        // (chost, cflags-hash) only.
        let chost = "riscv64-unknown-linux-gnu";
        let live: BTreeSet<Utf8PathBuf> = [Utf8PathBuf::from(chost).join("0123456789abcdef")]
            .into_iter()
            .collect();

        let entries = vec![
            entry(chost, "0123456789abcdef"), // live
            entry(chost, "fedcba9876543210"), // stale
        ];
        let unused = unused_entries(entries, &live);
        let keys: Vec<_> = unused.iter().map(|e| e.rel_key()).collect();
        assert_eq!(
            keys,
            vec![Utf8PathBuf::from(chost).join("fedcba9876543210")]
        );
    }

    #[test]
    fn empty_live_set_refuses_to_gc() {
        // No boards -> empty live set -> every store entry would classify
        // as unused; gc must refuse instead of wiping the global store.
        let live = LiveSet {
            store: BTreeSet::new(),
            binpkgs: BTreeSet::new(),
        };
        assert!(ensure_live_nonempty(&live, Utf8Path::new("/nowhere/boards")).is_err());

        let nonempty = LiveSet {
            store: [store_key(
                "riscv64-unknown-linux-gnu",
                "0123456789abcdef",
                "15",
            )]
            .into_iter()
            .collect(),
            binpkgs: BTreeSet::new(),
        };
        assert!(ensure_live_nonempty(&nonempty, Utf8Path::new("/nowhere/boards")).is_ok());
    }

    #[test]
    fn board_keys_use_gcc_version_verbatim() {
        let mut b = crate::cli::util::default_board_config("riscv64");
        b.gcc_version = Some("15.2.1_p20260214".into());
        let hash = crate::cflags::key_for_board(&b);
        assert_eq!(
            board_store_keys(&b, &["14".into()]),
            vec![store_key(&b.chost(), &hash, "15.2.1_p20260214")]
        );
    }

    #[test]
    fn board_keys_fall_back_to_sandbox_default_specs() {
        let b = crate::cli::util::default_board_config("riscv64");
        let hash = crate::cflags::key_for_board(&b);
        assert_eq!(
            board_store_keys(&b, &["14".into(), "15".into()]),
            vec![
                store_key(&b.chost(), &hash, "14"),
                store_key(&b.chost(), &hash, "15"),
            ]
        );
    }
}
