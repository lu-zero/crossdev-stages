use crate::cli::util::ensure_crossdev;
use crate::cli::ImageCmd;
use crate::error::Result;
use crate::{board, image, stage, target, workspace::Workspace};
use camino::{Utf8Path, Utf8PathBuf};

pub async fn run(
    ws: &Workspace,
    cmd: ImageCmd,
    boards_root: &Utf8Path,
    defaults_root: &Utf8Path,
    mirror: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    match cmd {
        ImageCmd::Build {
            board: board_name,
            sandbox,
            target,
            compression,
            steps,
        } => {
            let mut board_cfg = board::load(boards_root, &board_name)?;
            if let Some(c) = compression {
                board_cfg.compression = Some(c);
            }

            let default_steps: Vec<String> = if board_cfg.build_steps.is_empty() {
                [
                    "deps",
                    "checkout",
                    "bootloader",
                    "kernel",
                    "assemble",
                    "pack",
                ]
                .iter()
                .map(|s| s.to_string())
                .collect()
            } else {
                board_cfg.build_steps.clone()
            };
            let steps_to_show = if steps.is_empty() {
                &default_steps
            } else {
                &steps
            };

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
                    steps_to_show
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                return Ok(());
            }

            let sb = ensure_crossdev(
                ws,
                sandbox.as_deref(),
                &board_cfg.arch,
                &board_cfg,
                defaults_root,
                mirror,
                None,
            )
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

            let steps_opt = if steps.is_empty() {
                None
            } else {
                Some(steps.as_slice())
            };
            image::build(
                ws,
                &sb,
                &tgt,
                &board_cfg,
                boards_root,
                defaults_root,
                steps_opt,
            )?;
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
        ImageCmd::Export {
            board: board_name,
            output,
            all,
            tar,
        } => {
            let builds = ws.list_builds()?;
            let build = builds
                .iter()
                .filter_map(|dir| image::Build::open(dir.clone()))
                .find(|b| b.board == board_name)
                .ok_or_else(|| {
                    crate::error::Error::BoardNotFound(format!("no builds for '{board_name}'"))
                })?;

            let out_dir = output.unwrap_or_else(|| Utf8PathBuf::from("."));
            std::fs::create_dir_all(&out_dir)?;

            if all {
                let bundle_root = out_dir.join(format!("{board_name}-flash-bundle"));
                // Start clean so artifacts dropped from bundle.list don't linger.
                if bundle_root.exists() {
                    std::fs::remove_dir_all(&bundle_root)?;
                }
                let manifest = boards_root.join(&board_name).join("bundle.list");
                if manifest.is_file() {
                    copy_listed_artifacts(&build.dir, &bundle_root, &manifest)?;
                } else {
                    copy_build_artifacts(&build.dir, &bundle_root)?;
                }
                copy_flash_aux(&boards_root.join(&board_name), &bundle_root)?;
                if tar {
                    let archive = out_dir.join(format!("{board_name}-flash-bundle.tar.xz"));
                    let status = std::process::Command::new("tar")
                        .args(["-cf", archive.as_str(), "-I", "xz -T0", "-C",
                               out_dir.as_str(),
                               &format!("{board_name}-flash-bundle")])
                        .status()?;
                    if !status.success() {
                        return Err(crate::error::Error::CommandFailed {
                            code: status.code().unwrap_or(1),
                            reason: "tar -I 'xz -T0' failed".into(),
                        });
                    }
                    let size = std::fs::metadata(&archive).map(|m| m.len()).unwrap_or(0);
                    println!("{archive} ({:.1}M)", size as f64 / 1_048_576.0);
                } else {
                    println!("Bundle at {bundle_root}");
                }
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

/// Copy only the paths listed in `<board>/bundle.list` (one relative path
/// per line, `#` comments + blanks ignored).  Preserves directory structure.
///
/// The magic token `@image` expands to the packed image filename recorded in
/// the build's `.image` marker (its name carries a UTC timestamp, so it cannot
/// be hardcoded).  A leading `optional:` marks an entry that may be absent —
/// it is skipped with a warning.  Any other missing entry is a hard error, so
/// a dropped boot blob fails the export instead of silently shipping a broken
/// bundle.
fn copy_listed_artifacts(src: &Utf8Path, dst: &Utf8Path, manifest: &Utf8Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    let text = std::fs::read_to_string(manifest)?;
    for line in text.lines() {
        let raw = line.split('#').next().unwrap_or("").trim();
        if raw.is_empty() {
            continue;
        }
        let (optional, token) = match raw.strip_prefix("optional:") {
            Some(rest) => (true, rest.trim()),
            None => (false, raw),
        };
        if token.is_empty() {
            continue;
        }
        let rel = if token == "@image" {
            match std::fs::read_to_string(src.join(".image")) {
                Ok(name) => name.trim().to_string(),
                Err(_) if optional => {
                    eprintln!("bundle.list: no packed image (.image marker absent), skipping @image");
                    continue;
                }
                Err(_) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "bundle.list: @image requested but build has no .image marker (not packed)",
                    )
                    .into());
                }
            }
        } else {
            token.to_string()
        };
        let s = src.join(&rel);
        if !s.is_file() {
            if optional {
                eprintln!("bundle.list: optional {rel} missing, skipping");
                continue;
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("bundle.list: required artifact missing: {rel}"),
            )
            .into());
        }
        let d = dst.join(&rel);
        if let Some(parent) = d.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&s, &d)?;
    }
    Ok(())
}

/// Pull flash helpers (partition tables, fastboot.yaml, flash.sh) from
/// the board source dir into the bundle so the tarball is self-contained.
fn copy_flash_aux(board_dir: &Utf8Path, dst: &Utf8Path) -> Result<()> {
    for name in ["partition_4M.json", "partition_universal.json",
                 "fastboot.yaml", "flash.sh"] {
        let src = board_dir.join(name);
        if src.is_file() {
            std::fs::copy(&src, dst.join(name))?;
        }
    }
    Ok(())
}

/// Recursively copy a build directory's flash artifacts into `dst`.
/// Top-level dirs in `TOP_SKIP_DIRS` are excluded (gen/ staged rootfs,
/// linux/ kernel source build, tmp/ scratch, firmware/ source clone) —
/// these are already baked into the partition images.  Symlinks and
/// dotfiles are always skipped.
fn copy_build_artifacts(src: &Utf8Path, dst: &Utf8Path) -> Result<()> {
    const TOP_SKIP_DIRS: &[&str] = &["gen", "linux", "tmp", "firmware"];
    copy_build_artifacts_rec(src, dst, true, TOP_SKIP_DIRS)
}

fn copy_build_artifacts_rec(
    src: &Utf8Path,
    dst: &Utf8Path,
    is_top: bool,
    top_skip: &[&str],
) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name().into_string().unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        let ty = entry.file_type()?;
        if ty.is_symlink() {
            continue;
        }
        if ty.is_dir() {
            if is_top && top_skip.contains(&name.as_str()) {
                continue;
            }
            let Ok(s_utf8) = camino::Utf8PathBuf::try_from(entry.path()) else {
                continue; // non-UTF-8 path; skip like the file_name() branch above
            };
            copy_build_artifacts_rec(&s_utf8, &dst.join(&name), false, top_skip)?;
        } else if ty.is_file() {
            std::fs::copy(entry.path(), dst.join(&name))?;
        }
    }
    Ok(())
}
