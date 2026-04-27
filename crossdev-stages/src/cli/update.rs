use std::collections::BTreeMap;
use std::process::Command;

use camino::{Utf8Path, Utf8PathBuf};
use serde::Deserialize;

use crate::error::Result;
use crate::workspace::Workspace;

#[derive(Debug, Deserialize)]
struct LockFile {
    build: BuildMeta,
    #[serde(default)]
    sources: BTreeMap<String, LockSource>,
}

#[derive(Debug, Deserialize)]
struct BuildMeta {
    board: String,
}

#[derive(Debug, Deserialize)]
struct LockSource {
    repo: String,
    tag: String,
    commit: String,
    #[serde(default)]
    kind: Option<String>,
}

pub fn run(ws: &Workspace, board: Option<&str>, all: bool) -> Result<()> {
    let lock_paths = if all {
        locate_all_locks(ws)
    } else {
        vec![locate_lock(ws, board)?]
    };
    if lock_paths.is_empty() {
        println!("No usable build.lock.toml found.");
        return Ok(());
    }

    for (i, lock_path) in lock_paths.iter().enumerate() {
        if i > 0 {
            println!();
        }
        check_lock(ws, lock_path)?;
    }
    Ok(())
}

fn check_lock(ws: &Workspace, lock_path: &Utf8Path) -> Result<()> {
    let body = std::fs::read_to_string(lock_path).map_err(|e| {
        crate::error::Error::CommandFailed {
            code: 1,
            reason: format!("read {lock_path}: {e}"),
        }
    })?;
    let lock: LockFile = toml::from_str(&body).map_err(|e| {
        crate::error::Error::CommandFailed {
            code: 1,
            reason: format!("parse {lock_path}: {e}"),
        }
    })?;

    println!("Locked build: {}", lock_path);
    println!("Board: {}\n", lock.build.board);

    let sources_dir = ws.sources_dir();
    let mut behind = 0;
    let mut clean = 0;
    let mut errored = 0;

    for (name, src) in &lock.sources {
        if src.kind.as_deref() != Some("git") {
            continue;
        }
        let cache = repo_cache_dir(&sources_dir, &src.repo, name);
        match check_source(name, src, &cache) {
            Ok(SourceState::UpToDate) => clean += 1,
            Ok(SourceState::Behind { count, head, log }) => {
                behind += 1;
                println!("[{}] {} -> {}  ({} new commit{})",
                    name,
                    short(&src.commit),
                    short(&head),
                    count,
                    if count == 1 { "" } else { "s" });
                println!("    repo: {}", src.repo);
                println!("    tag:  {}", src.tag);
                for line in log.lines().take(5) {
                    println!("    {line}");
                }
                if log.lines().count() > 5 {
                    println!("    ...");
                }
            }
            Err(e) => {
                errored += 1;
                println!("[{}] error: {e}", name);
            }
        }
    }

    println!(
        "Summary: {behind} updatable, {clean} up-to-date, {errored} error{}",
        if errored == 1 { "" } else { "s" },
    );
    Ok(())
}

enum SourceState {
    UpToDate,
    Behind { count: usize, head: String, log: String },
}

fn check_source(name: &str, src: &LockSource, cache: &Utf8Path) -> std::result::Result<SourceState, String> {
    if !cache.exists() {
        return Err(format!("no cache at {cache} (run `image build` first)"));
    }

    // The tag may be a tag, a branch, or both (LTS-style branches that
    // get cut as tags).  Try each refspec independently; ignore the
    // "couldn't find remote ref" failure for whichever doesn't exist.
    for refspec in [
        format!("refs/tags/{tag}:refs/tags/{tag}", tag = src.tag),
        format!("refs/heads/{tag}:refs/heads/{tag}", tag = src.tag),
    ] {
        let _ = Command::new("git")
            .args(["-C", cache.as_str(), "fetch", "--quiet", &src.repo, &refspec])
            .output();
    }

    // Dereference annotated tags to the commit they point to; on a
    // lightweight tag or branch this is a no-op.
    let head = run_git(cache, &["rev-parse", &format!("{}^{{commit}}", src.tag)])
        .map_err(|e| format!("rev-parse {} ({name}): {e}", src.tag))?
        .trim()
        .to_string();

    if head == src.commit {
        return Ok(SourceState::UpToDate);
    }

    let count_str = run_git(
        cache,
        &["rev-list", "--count", &format!("{}..{}", src.commit, head)],
    )
    .map_err(|e| format!("rev-list ({name}): {e}"))?;
    let count: usize = count_str.trim().parse().unwrap_or(0);

    let log = run_git(
        cache,
        &[
            "log", "--oneline", "--no-decorate",
            &format!("{}..{}", src.commit, head),
        ],
    )
    .unwrap_or_default();

    Ok(SourceState::Behind { count, head, log })
}

fn run_git(cache: &Utf8Path, args: &[&str]) -> std::result::Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(cache.as_str()).args(args);
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn locate_lock(ws: &Workspace, board: Option<&str>) -> Result<Utf8PathBuf> {
    let builds = ws.list_builds()?;
    for dir in builds {
        let lock = dir.join("build.lock.toml");
        if !lock.is_file() {
            continue;
        }
        if let Some(want) = board {
            let on_disk = std::fs::read_to_string(dir.join(".board"))
                .ok()
                .map(|s| s.trim().to_string());
            if on_disk.as_deref() != Some(want) {
                continue;
            }
        }
        // Skip incomplete builds whose lock has no resolved git sources
        // (every kind=missing) -- nothing useful to compare against.
        if !lock_has_git_sources(&lock) {
            continue;
        }
        return Ok(lock);
    }
    Err(crate::error::Error::CommandFailed {
        code: 1,
        reason: match board {
            Some(b) => format!("no build.lock.toml with git sources for board '{b}'"),
            None => "no build.lock.toml with git sources in any build".into(),
        },
    })
}

/// Pick one usable lock per board (newest by build dir order, since
/// `list_builds` returns newest first).  Skips locks that lack any
/// resolved git source.
fn locate_all_locks(ws: &Workspace) -> Vec<Utf8PathBuf> {
    let Ok(builds) = ws.list_builds() else { return vec![] };
    let mut seen = std::collections::BTreeSet::<String>::new();
    let mut out = Vec::new();
    for dir in builds {
        let Ok(board) = std::fs::read_to_string(dir.join(".board"))
            .map(|s| s.trim().to_string())
        else { continue };
        if seen.contains(&board) {
            continue;
        }
        let lock = dir.join("build.lock.toml");
        if !lock.is_file() || !lock_has_git_sources(&lock) {
            continue;
        }
        seen.insert(board);
        out.push(lock);
    }
    out
}

fn lock_has_git_sources(lock_path: &Utf8Path) -> bool {
    let Ok(body) = std::fs::read_to_string(lock_path) else { return false };
    let Ok(parsed) = toml::from_str::<LockFile>(&body) else { return false };
    parsed.sources.values().any(|s| s.kind.as_deref() == Some("git") && !s.commit.is_empty())
}

fn short(commit: &str) -> &str {
    if commit.len() > 12 { &commit[..12] } else { commit }
}

/// Mirror of source_cache::repo_cache_name -> /sources/<n>.git on host.
fn repo_cache_dir(sources_dir: &Utf8Path, repo: &str, fallback: &str) -> Utf8PathBuf {
    let stripped = repo.trim_end_matches('/').trim_end_matches(".git");
    let name = if let Some(idx) = stripped.rfind("://") {
        let path = &stripped[idx + 3..];
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() >= 2 {
            format!("{}-{}", parts[parts.len() - 2], parts[parts.len() - 1])
        } else {
            fallback.to_string()
        }
    } else {
        fallback.to_string()
    };
    sources_dir.join(format!("{name}.git"))
}
