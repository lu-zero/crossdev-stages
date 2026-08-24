use crate::cli::util::{ensure_crossdev, ensure_sandbox};
use crate::cli::ImageCmd;
use crate::error::{Error, Result};
use crate::{board, image, stage, target, workspace::Workspace};
use camino::{Utf8Path, Utf8PathBuf};
use serde::Deserialize;
use std::collections::BTreeMap;

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

            let default_steps: Vec<String> = board_cfg
                .effective_build_steps()
                .iter()
                .map(|s| s.to_string())
                .collect();
            let steps_to_show = if steps.is_empty() {
                &default_steps
            } else {
                &steps
            };

            if dry_run {
                let tags = if board_cfg.tags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", board_cfg.tags.join(","))
                };
                println!("Board:      {}{tags}", board_cfg.name);
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

            let provider = board_cfg.rootfs_provider;
            let needs_toolchain = provider.needs_cross_toolchain(
                &steps_to_show
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            );
            let sb = if needs_toolchain {
                ensure_crossdev(
                    ws,
                    sandbox.as_deref(),
                    &board_cfg.arch,
                    &board_cfg,
                    defaults_root,
                    mirror,
                    None,
                )
                .await?
            } else {
                ensure_sandbox(ws, sandbox.as_deref(), defaults_root, mirror).await?
            };

            let tgt = match ws.resolve_target_for_arch(
                target.as_deref(),
                &board_cfg.arch,
                provider.name(),
            ) {
                Ok(td) => target::Target::open(td)?,
                Err(_) => {
                    // Non-Gentoo providers get a provider-qualified default
                    // name so they never collide with the plain `<arch>`
                    // stage3 target of a Gentoo board on the same arch.
                    let default_name = if provider.provisions_stage3() {
                        board_cfg.arch.clone()
                    } else {
                        format!("{}-{}", board_cfg.arch, provider.name())
                    };
                    let name = target.as_deref().unwrap_or(&default_name).to_string();
                    if provider.provisions_stage3() {
                        tracing::info!("Target '{name}' not found, creating from stage3…");
                        let source_stage =
                            stage::fetch(&ws.stages_dir(), &board_cfg.arch, mirror).await?;
                        target::Target::create(ws, &name, &board_cfg.arch, &source_stage)?
                    } else {
                        tracing::info!(
                            "Target '{name}' not found, creating empty (rootfs provider fills it)…"
                        );
                        target::Target::create_empty(ws, &name, &board_cfg.arch, provider.name())?
                    }
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
                    let digest = write_sha256(&archive)?;
                    let size = std::fs::metadata(&archive).map(|m| m.len()).unwrap_or(0);
                    println!("{archive} ({:.1}M)", size as f64 / 1_048_576.0);
                    println!("{digest}  {}", archive.file_name().unwrap_or_default());
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
                        let digest = write_sha256(&dest)?;
                        let size = std::fs::metadata(&src).map(|m| m.len()).unwrap_or(0);
                        println!("{name} ({:.1}M) -> {dest}", size as f64 / 1_048_576.0);
                        println!("{digest}  {name}");
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

/// Hash `file` and write `<file>.sha256` beside it, returning the hex digest.
///
/// Written in the format `sha256sum -c` expects, with the bare filename rather
/// than the path, so the check works from whichever directory the image is
/// carried to.
fn write_sha256(file: &Utf8Path) -> Result<String> {
    use sha2::{Digest, Sha256};

    use std::io::Read;

    let mut reader = std::fs::File::open(file)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let read = reader.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    let digest: String = hasher.finalize().iter().fold(String::new(), |mut acc, byte| {
        use std::fmt::Write;
        let _ = write!(acc, "{byte:02x}");
        acc
    });

    let name = file.file_name().unwrap_or_default();
    std::fs::write(
        format!("{file}.sha256"),
        format!("{digest}  {name}\n"),
    )?;
    Ok(digest)
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
fn apply_pin_overrides(
    ws: &Workspace,
    board_name: &str,
    board_cfg: &mut board::BoardConfig,
) -> Result<()> {
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
    if let Some(src) = lock.sources.get("syslinux").and_then(pin) {
        if board_cfg.syslinux_repo.is_some() {
            board_cfg.syslinux_tag = Some(src);
            applied += 1;
        }
    }
    // firmware defaults to the U-Boot tag; pin it explicitly so a pinned
    // u_boot_tag (a U-Boot SHA) never leaks into the firmware checkout.
    if let Some(src) = lock.sources.get("firmware").and_then(pin) {
        if board_cfg.firmware_repo.is_some() {
            board_cfg.firmware_tag = Some(src);
            applied += 1;
        }
    }
    if let Some(src) = lock.sources.get("buildroot").and_then(pin) {
        board_cfg.buildroot_tag = Some(src);
        applied += 1;
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
                if parsed
                    .sources
                    .values()
                    .any(|s| s.kind.as_deref() == Some("git") && !s.commit.is_empty())
                {
                    return Some(lock);
                }
            }
        }
    }
    None
}
