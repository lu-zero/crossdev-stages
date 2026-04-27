use std::collections::BTreeMap;

use camino::{Utf8Path, Utf8PathBuf};
use serde::Deserialize;

use crate::{board, image, stage, target, workspace::Workspace};
use crate::error::{Error, Result};
use crate::cli::ImageCmd;
use crate::cli::util::ensure_crossdev;

pub async fn run(
    ws: &Workspace,
    cmd: ImageCmd,
    boards_root: &Utf8Path,
    mirror: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    match cmd {
        ImageCmd::Build {
            board: board_name,
            sandbox,
            target,
            compression,
            pinned,
            steps,
        } => {
            let mut board_cfg = board::load(boards_root, &board_name)?;
            if let Some(c) = compression {
                board_cfg.compression = Some(c);
            }
            if pinned {
                apply_pin_overrides(ws, &board_name, &mut board_cfg)?;
            }

            let default_steps: Vec<String> = if board_cfg.build_steps.is_empty() {
                ["deps", "checkout", "bootloader", "kernel", "assemble", "pack"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            } else {
                board_cfg.build_steps.clone()
            };
            let steps_to_show = if steps.is_empty() { &default_steps } else { &steps };

            if dry_run {
                let tag = if board_cfg.testing { " [TESTING]" } else { "" };
                println!("Board:      {}{tag}", board_cfg.name);
                println!("Arch:       {}", board_cfg.arch);
                println!("CFLAGS:     {}", board_cfg.effective_cflags());
                if let Some(ldflags) = &board_cfg.ldflags {
                    println!("LDFLAGS:    {ldflags}");
                }
                if let Some(rustflags) = &board_cfg.rustflags {
                    println!("RUSTFLAGS:  {rustflags}");
                }
                println!(
                    "Steps:      {}",
                    steps_to_show.iter().map(String::as_str).collect::<Vec<_>>().join(" ")
                );
                return Ok(());
            }

            let sb =
                ensure_crossdev(ws, sandbox.as_deref(), &board_cfg.arch, &board_cfg, mirror)
                    .await?;

            let tgt = match ws.resolve_target_for_arch(target.as_deref(), &board_cfg.arch) {
                Ok(td) => target::Target::open(td)?,
                Err(_) => {
                    let name = target.as_deref().unwrap_or(&board_cfg.arch).to_string();
                    tracing::info!("Target '{name}' not found, creating from stage3…");
                    let source_stage =
                        stage::fetch(&ws.stages_dir(), &board_cfg.arch, mirror).await?;
                    target::Target::create(ws, &name, &board_cfg.arch, &source_stage)?
                }
            };

            let steps_opt = if steps.is_empty() { None } else { Some(steps.as_slice()) };
            image::build(ws, &sb, &tgt, &board_cfg, boards_root, steps_opt)?;
        }
        ImageCmd::Prune => {
            let builds = ws.list_builds()?;
            let mut pruned = 0;
            for dir in builds {
                if !dir.join(".packed").exists() {
                    std::fs::remove_dir_all(&dir)?;
                    pruned += 1;
                }
            }
            println!("Pruned {pruned} incomplete build(s).");
        }
        ImageCmd::Export { board: board_name, output, all } => {
            let builds = ws.list_builds()?;
            let build = builds
                .iter()
                .filter_map(|dir| image::Build::open(dir.clone()))
                .find(|b| b.board == board_name)
                .ok_or_else(|| crate::error::Error::BoardNotFound(
                    format!("no builds for '{board_name}'"),
                ))?;

            let out_dir = output.unwrap_or_else(|| Utf8PathBuf::from("."));
            std::fs::create_dir_all(&out_dir)?;

            if all {
                let mut exported = 0;
                for entry in std::fs::read_dir(&build.dir)? {
                    let entry = entry?;
                    let name = entry.file_name().into_string().unwrap_or_default();
                    if name.starts_with('.') || !entry.path().is_file() {
                        continue;
                    }
                    let dest = out_dir.join(&name);
                    std::fs::copy(entry.path(), &dest)?;
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    println!("{name} ({:.1}M)", size as f64 / 1_048_576.0);
                    exported += 1;
                }
                println!("{exported} file(s) exported to {out_dir}");
            } else {
                let img_name = std::fs::read_to_string(build.dir.join(".image"))
                    .map(|s| s.trim().to_string())
                    .ok();
                if let Some(name) = img_name {
                    let src = build.dir.join(&name);
                    if src.is_file() {
                        let dest = out_dir.join(&name);
                        std::fs::copy(&src, &dest)?;
                        let size = std::fs::metadata(&src).map(|m| m.len()).unwrap_or(0);
                        println!("{name} ({:.1}M) -> {dest}", size as f64 / 1_048_576.0);
                    } else {
                        println!("Image file missing: {src}");
                    }
                } else {
                    println!("Build not packed yet. Run: crossdev-stages image build --board {board_name}");
                }
            }
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct PinnedLock {
    #[serde(default)]
    sources: BTreeMap<String, PinnedSource>,
}

#[derive(Deserialize)]
struct PinnedSource {
    commit: String,
    #[serde(default)]
    kind: Option<String>,
}

/// Replace each TAG field on `board_cfg` with the commit recorded in the
/// most recent build.lock.toml for `board_name`, so the build resolves to
/// exactly the sources that were used last time even if upstream branches
/// have advanced.  Unknown sources or missing locks are ignored: the
/// board's own TAG stays as a fallback.
fn apply_pin_overrides(ws: &Workspace, board_name: &str, board_cfg: &mut board::BoardConfig) -> Result<()> {
    let Some(lock_path) = newest_usable_lock(ws, board_name) else {
        return Err(Error::CommandFailed {
            code: 1,
            reason: format!("--pinned: no usable build.lock.toml for board '{board_name}'"),
        });
    };
    let body = std::fs::read_to_string(&lock_path)?;
    let lock: PinnedLock = toml::from_str(&body).map_err(|e| Error::CommandFailed {
        code: 1,
        reason: format!("parse {lock_path}: {e}"),
    })?;
    let pin = |s: &PinnedSource| -> Option<String> {
        if s.kind.as_deref() == Some("git") && !s.commit.is_empty() {
            Some(s.commit.clone())
        } else {
            None
        }
    };
    let mut applied = 0;
    if let Some(src) = lock.sources.get("opensbi").and_then(pin) {
        if board_cfg.opensbi_repo.is_some() {
            board_cfg.opensbi_tag = Some(src);
            applied += 1;
        }
    }
    if let Some(src) = lock.sources.get("uboot").and_then(pin) {
        if board_cfg.u_boot_repo.is_some() {
            board_cfg.u_boot_tag = Some(src);
            applied += 1;
        }
    }
    if let Some(src) = lock.sources.get("tfa").and_then(pin) {
        if board_cfg.tfa_repo.is_some() {
            board_cfg.tfa_tag = Some(src);
            applied += 1;
        }
    }
    if let Some(src) = lock.sources.get("rkbin").and_then(pin) {
        if board_cfg.rkbin_repo.is_some() {
            board_cfg.rkbin_tag = Some(src);
            applied += 1;
        }
    }
    if let Some(src) = lock.sources.get("firmware").and_then(pin) {
        if board_cfg.firmware_repo.is_some() {
            board_cfg.firmware_tag = Some(src);
            applied += 1;
        }
    }
    if let Some(src) = lock.sources.get("kernel").and_then(pin) {
        board_cfg.kernel_tag = src;
        applied += 1;
    }
    tracing::info!(
        "Pinned {applied} source(s) from {lock_path}; rebuilding board '{board_name}' against locked commits"
    );
    Ok(())
}

fn newest_usable_lock(ws: &Workspace, board_name: &str) -> Option<Utf8PathBuf> {
    let builds = ws.list_builds().ok()?;
    for dir in builds {
        let on_disk = std::fs::read_to_string(dir.join(".board"))
            .ok()
            .map(|s| s.trim().to_string());
        if on_disk.as_deref() != Some(board_name) {
            continue;
        }
        let lock = dir.join("build.lock.toml");
        if !lock.is_file() {
            continue;
        }
        if let Ok(body) = std::fs::read_to_string(&lock) {
            if let Ok(parsed) = toml::from_str::<PinnedLock>(&body) {
                if parsed.sources.values().any(|s| s.kind.as_deref() == Some("git") && !s.commit.is_empty()) {
                    return Some(lock);
                }
            }
        }
    }
    None
}
