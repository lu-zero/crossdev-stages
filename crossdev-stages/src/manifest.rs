//! Build provenance and image manifests.  Two emitters:
//!
//! - [`ManifestBuilder`] → `build.lock.toml` in the build dir: source
//!   commits, stage3, toolchain CFLAGS, config hashes.  Observability,
//!   with one exception: a source pinned to a commit SHA that the built
//!   tree is not on fails the build (see `pin_mismatches`).
//! - [`write_image_sidecar`] → `<image>.manifest.json` next to the packed
//!   image: full-image sha256 + partition table (offset/size/source/sha256)
//!   for verifying integrity and dd-ing partitions to eMMC/SPI flash at
//!   known offsets without re-parsing genimage.cfg.

use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::BTreeMap;
use std::process::Command;

use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::Result;

/// What went into a single image build. Written as `build.lock.toml` in the
/// build dir at pipeline end.
#[derive(Debug, Serialize)]
pub struct BuildManifest {
    pub build: BuildMeta,
    pub stage3: Stage3Info,
    pub toolchain: Toolchain,
    pub sources: BTreeMap<String, SourceEntry>,
    pub configs: Configs,
}

#[derive(Debug, Serialize)]
pub struct BuildMeta {
    pub board: String,
    pub arch: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub crossdev_stages_commit: String,
}

#[derive(Debug, Serialize)]
pub struct Stage3Info {
    pub file: String,
}

#[derive(Debug, Serialize)]
pub struct Toolchain {
    pub crossdev_prefix_cflags: String,
    pub crossdev_prefix_cxxflags: String,
    pub target_cflags: String,
}

#[derive(Debug, Serialize)]
pub struct SourceEntry {
    pub repo: String,
    pub tag: String,
    /// For `kind = "git"`: resolved git commit sha.
    /// For `kind = "local"`: sha256 of the tree (recursive, sorted).
    /// For `kind = "missing"`: empty.
    pub commit: String,
    pub kind: SourceKind,
    pub path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Git,
    Local,
    Missing,
}

#[derive(Debug, Serialize)]
pub struct Configs {
    pub kernel_config_sha256: Option<String>,
    pub uboot_config_sha256: Option<String>,
}

/// Collector state during the build; finalized by `write()`.
pub struct ManifestBuilder {
    started_at: DateTime<Utc>,
    board: String,
    arch: String,
    chost: String,
    sources: BTreeMap<String, SourceEntry>,
}

impl ManifestBuilder {
    pub fn new(board: &BoardConfig) -> Self {
        Self {
            started_at: Utc::now(),
            board: board.name.clone(),
            arch: board.arch.clone(),
            chost: board.chost(),
            sources: BTreeMap::new(),
        }
    }

    /// Whether at least one recorded source resolved to a real git tree
    /// (i.e. it was actually checked out before record_sources ran).
    /// Image::build skips writing the lock when this is false to avoid
    /// publishing a useless "all kind=missing" manifest from a partial
    /// `--steps deps` invocation.
    pub fn has_resolved_source(&self) -> bool {
        self.sources
            .values()
            .any(|s| matches!(s.kind, SourceKind::Git))
    }

    /// Record a source. `path` is the inside-sandbox path (e.g. `/build/linux`).
    /// Best-effort resolution:
    /// - `.git` present → `git rev-parse HEAD`, `kind = git`
    /// - directory present, no `.git` → sha256 of tree contents, `kind = local`
    /// - absent → `kind = missing` (still recorded, so the lock's shape stays
    ///   stable per board)
    pub fn record_source(
        &mut self,
        runner: &SandboxRunner,
        name: &str,
        repo: &str,
        tag: &str,
        path: &str,
    ) -> Result<()> {
        let probe = runner.run_output(&format!(
            "if [ -d {path}/.git ]; then \
                echo git; git -C {path} rev-parse HEAD; \
             elif [ -d {path} ]; then \
                echo local; \
                find {path} -type f -print0 | sort -z | xargs -0 sha256sum 2>/dev/null \
                    | sha256sum | cut -d' ' -f1; \
             else \
                echo missing; echo; \
             fi"
        ))?;
        let mut lines = probe.lines();
        let kind_str = lines.next().unwrap_or("").trim();
        let commit = lines.next().unwrap_or("").trim().to_string();
        let kind = match kind_str {
            "git" => SourceKind::Git,
            "local" => SourceKind::Local,
            _ => SourceKind::Missing,
        };
        self.sources.insert(
            name.to_string(),
            SourceEntry {
                repo: repo.to_string(),
                tag: tag.to_string(),
                commit,
                kind,
                path: path.to_string(),
            },
        );
        Ok(())
    }

    /// Sources whose recorded tag is a full commit SHA the built tree does
    /// not sit on.  A 40-hex tag names one tree and nothing else, so tag and
    /// commit are the same quantity read twice: what the build was told to
    /// check out, and what `git rev-parse HEAD` found in the tree the build
    /// compiled.  They cannot legitimately disagree -- a skipped checkout, or
    /// a SHA pinned onto the wrong repo, and the image is not what the lock
    /// says it is.  A named tag or branch makes no such claim and is skipped.
    pub fn pin_mismatches(&self) -> Vec<String> {
        self.sources
            .iter()
            .filter(|(_, s)| matches!(s.kind, SourceKind::Git))
            .filter(|(_, s)| {
                crate::source_cache::is_commit_sha(&s.tag) && !s.tag.eq_ignore_ascii_case(&s.commit)
            })
            .map(|(name, s)| {
                format!(
                    "source '{name}' pinned to {} but the tree that was built is at {}",
                    s.tag, s.commit
                )
            })
            .collect()
    }

    /// Gather toolchain CFLAGS by reading the two relevant make.conf files
    /// inside the sandbox.
    fn read_toolchain(&self, runner: &SandboxRunner) -> Result<Toolchain> {
        let prefix_mk = format!("/usr/{}/etc/portage/make.conf", self.chost);
        let target_mk = "/target/etc/portage/make.conf";

        Ok(Toolchain {
            crossdev_prefix_cflags: read_makeconf_var(runner, &prefix_mk, "CFLAGS"),
            crossdev_prefix_cxxflags: read_makeconf_var(runner, &prefix_mk, "CXXFLAGS"),
            target_cflags: read_makeconf_var(runner, target_mk, "CFLAGS"),
        })
    }

    fn read_configs(&self, runner: &SandboxRunner) -> Configs {
        Configs {
            kernel_config_sha256: sha256_of(runner, "/build/linux/.config"),
            uboot_config_sha256: sha256_of(runner, "/build/u-boot/.config"),
        }
    }

    pub fn write(self, runner: &SandboxRunner, out_path: &Utf8Path) -> Result<Utf8PathBuf> {
        let toolchain = self.read_toolchain(runner)?;
        let configs = self.read_configs(runner);
        // Read the `.stage3` marker Target::create writes, falling back
        // to "newest tarball under /cache/stages matching this arch"
        // (covers targets created before the marker existed).
        let stage3_file = runner
            .run_output("cat /target/.stage3 2>/dev/null || true")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                let arch = gentoo_arch_dir(&self.arch);
                runner
                    .run_output(&format!(
                        "ls -t /cache/stages/*/{arch}/stage3-{arch}-*.tar.* \
                                  /cache/stages/stage3-{arch}-*.tar.* 2>/dev/null \
                         | head -1 | xargs -r basename || true"
                    ))
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_default();
        let manifest = BuildManifest {
            build: BuildMeta {
                board: self.board,
                arch: self.arch,
                started_at: self.started_at,
                finished_at: Utc::now(),
                crossdev_stages_commit: crossdev_stages_commit().to_string(),
            },
            stage3: Stage3Info { file: stage3_file },
            toolchain,
            sources: self.sources,
            configs,
        };
        let body =
            toml::to_string_pretty(&manifest).map_err(|e| crate::error::Error::CommandFailed {
                code: 1,
                reason: format!("toml serialize failed: {e}"),
            })?;
        std::fs::write(out_path, body)?;
        Ok(out_path.to_path_buf())
    }
}

/// Effective value of a make.conf variable: sourced so `${COMMON_FLAGS}`-style
/// references expand to what Portage would actually use at emerge time.
fn read_makeconf_var(runner: &SandboxRunner, file: &str, name: &str) -> String {
    let cmd = format!(
        "[ -f {file} ] && (set -a; . {file} 2>/dev/null; printf '%s' \"${{{name}:-}}\") || true"
    );
    runner
        .run_output(&cmd)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn sha256_of(runner: &SandboxRunner, file: &str) -> Option<String> {
    let cmd = format!("[ -f {file} ] && sha256sum {file} | cut -d' ' -f1 || true");
    runner.run_output(&cmd).ok().and_then(|s| {
        let s = s.trim();
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    })
}

/// Embedded at compile time via `build.rs`; falls back to "unknown" if absent.
fn crossdev_stages_commit() -> &'static str {
    option_env!("CROSSDEV_STAGES_GIT_COMMIT").unwrap_or("unknown")
}

/// Gentoo's arch directory name differs from uname-style arch in a few cases.
fn gentoo_arch_dir(arch: &str) -> &str {
    match arch {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        other => other,
    }
}

// ── Image sidecar ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ImageSidecar {
    pub board: String,
    pub image: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub built_at: String,
    pub partitions: Vec<Partition>,
}

#[derive(Serialize)]
pub struct Partition {
    pub name: String,
    pub offset: Option<String>,
    pub size: Option<String>,
    pub image: Option<String>,
    pub sha256: Option<String>,
}

pub fn write_image_sidecar(
    build_dir: &Utf8Path,
    board: &str,
    image_name: &str,
    genimage_cfg: Option<&Utf8Path>,
) -> Result<()> {
    let img_path = build_dir.join(image_name);
    let size_bytes = std::fs::metadata(&img_path)?.len();
    let sha256 = sha256_file(&img_path)?;

    let mut partitions = match genimage_cfg {
        Some(p) if p.exists() => parse_partitions(&std::fs::read_to_string(p)?),
        _ => Vec::new(),
    };
    for part in &mut partitions {
        if let Some(src) = &part.image {
            let abs = build_dir.join(src);
            if abs.exists() {
                part.sha256 = Some(sha256_file(&abs)?);
            }
        }
    }

    let manifest = ImageSidecar {
        board: board.to_string(),
        image: image_name.to_string(),
        size_bytes,
        sha256,
        built_at: chrono::Utc::now().to_rfc3339(),
        partitions,
    };

    let out = build_dir.join(format!("{image_name}.manifest.json"));
    std::fs::write(&out, serde_json::to_string_pretty(&manifest)?)?;
    Ok(())
}

fn sha256_file(path: &Utf8Path) -> Result<String> {
    let output = Command::new("sha256sum").arg(path.as_str()).output()?;
    if !output.status.success() {
        return Err(crate::error::Error::CommandFailed {
            code: output.status.code().unwrap_or(1),
            reason: format!(
                "sha256sum {path}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    output
        .stdout
        .split(|&b| b == b' ')
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).to_string())
        .ok_or_else(|| crate::error::Error::CommandFailed {
            code: 1,
            reason: format!("sha256sum {path}: empty output"),
        })
}

/// Parse `partition NAME { key = "value" ... }` blocks from every
/// `image NAME { ... }` block of a genimage config.  (genimage allows
/// multiple top-level images — rootfs.ext4, bootfs, then the final
/// hdimage with partitions.)
fn parse_partitions(cfg: &str) -> Vec<Partition> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut in_image = false;
    let mut current: Option<Partition> = None;

    for raw in cfg.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("/*") {
            continue;
        }

        if !in_image && line.starts_with("image ") && line.ends_with('{') {
            in_image = true;
            depth = 1;
            continue;
        }
        if !in_image {
            continue;
        }

        if let Some(name) = line
            .strip_prefix("partition ")
            .and_then(|s| s.strip_suffix(" {"))
        {
            current = Some(Partition {
                name: name.trim().to_string(),
                offset: None,
                size: None,
                image: None,
                sha256: None,
            });
            depth += 1;
            continue;
        }

        if line == "}" {
            depth -= 1;
            if let Some(p) = current.take() {
                out.push(p);
            }
            if depth == 0 {
                in_image = false;
            }
            continue;
        }

        if line.ends_with('{') {
            depth += 1;
            continue;
        }

        if let Some(p) = current.as_mut() {
            if let Some((k, v)) = line.split_once('=') {
                let v = v
                    .trim()
                    .trim_matches('"')
                    .trim_end_matches(';')
                    .trim_matches('"');
                match k.trim() {
                    "offset" => p.offset = Some(v.to_string()),
                    "size" if !v.is_empty() => p.size = Some(v.to_string()),
                    "image" => p.image = Some(v.to_string()),
                    _ => {}
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builder_with(entries: &[(&str, &str, &str, SourceKind)]) -> ManifestBuilder {
        let mut b = ManifestBuilder::new(&crate::cli::util::default_board_config("aarch64"));
        for (name, tag, commit, kind) in entries {
            b.sources.insert(
                (*name).to_string(),
                SourceEntry {
                    repo: "https://example.invalid/repo.git".into(),
                    tag: (*tag).to_string(),
                    commit: (*commit).to_string(),
                    kind: match kind {
                        SourceKind::Git => SourceKind::Git,
                        SourceKind::Local => SourceKind::Local,
                        SourceKind::Missing => SourceKind::Missing,
                    },
                    path: format!("/build/{name}"),
                },
            );
        }
        b
    }

    /// The lock a real build wrote on 2026-08-25: the kernel step was skipped
    /// by its resume marker, so the pin never reached the tree.
    #[test]
    fn pinned_sha_against_a_stale_tree_is_a_mismatch() {
        let b = builder_with(&[(
            "kernel",
            "4e69c1856bfd9ffb7e9d335a25842fa211628929",
            "8d3ae59288f1e7d58d76558a6ee96d533bc5019f",
            SourceKind::Git,
        )]);
        let found = b.pin_mismatches();
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("kernel"));
    }

    #[test]
    fn pinned_sha_that_matches_is_clean() {
        let sha = "4e69c1856bfd9ffb7e9d335a25842fa211628929";
        assert!(builder_with(&[("kernel", sha, sha, SourceKind::Git)])
            .pin_mismatches()
            .is_empty());
        // git prints lowercase; a hand-edited board.conf may not.
        let upper = sha.to_ascii_uppercase();
        assert!(builder_with(&[("kernel", &upper, sha, SourceKind::Git)])
            .pin_mismatches()
            .is_empty());
    }

    /// A name resolves to whatever it points at today, so it claims nothing
    /// the commit could contradict.
    #[test]
    fn named_tags_and_branches_claim_nothing() {
        for tag in ["v7.2", "master", "linux-6.6.y"] {
            assert!(builder_with(&[(
                "kernel",
                tag,
                "8d3ae59288f1e7d58d76558a6ee96d533bc5019f",
                SourceKind::Git,
            )])
            .pin_mismatches()
            .is_empty());
        }
    }

    /// commit is a tree hash for local and empty for missing; neither is a
    /// git sha and neither can be compared to one.
    #[test]
    fn non_git_sources_are_not_compared() {
        let sha = "4e69c1856bfd9ffb7e9d335a25842fa211628929";
        assert!(builder_with(&[("kernel", sha, "deadbeef", SourceKind::Local)])
            .pin_mismatches()
            .is_empty());
        assert!(builder_with(&[("kernel", sha, "", SourceKind::Missing)])
            .pin_mismatches()
            .is_empty());
    }

    #[test]
    fn skips_preceding_filesystem_images() {
        let cfg = r#"
image rootfs.ext4 {
    ext4 { label = "rootfs" }
    size = 5G
}

image bootfs.fat32 {
    vfat { label = "boot" }
    size = 128M
}

image sdcard.img {
    hdimage { partition-table-type = gpt }
    partition rootfs {
        image = "rootfs.ext4"
        offset = "131M"
    }
}
"#;
        let parts = parse_partitions(cfg);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].name, "rootfs");
    }

    #[test]
    fn parses_k230_layout() {
        let cfg = r#"
image foo.img {
    hdimage { partition-table-type = gpt }
    partition uboot_spl_1 {
        image = "u-boot/fn_u-boot-spl.bin"
        offset = "1024K"
        size = "512K"
    }
    partition rootfs {
        image = "rootfs.ext4"
        offset = "131M"
        size = ""
        partition-type-uuid = "root-riscv64"
    }
}
"#;
        let parts = parse_partitions(cfg);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].name, "uboot_spl_1");
        assert_eq!(parts[0].offset.as_deref(), Some("1024K"));
        assert_eq!(parts[0].image.as_deref(), Some("u-boot/fn_u-boot-spl.bin"));
        assert_eq!(parts[1].name, "rootfs");
        assert_eq!(parts[1].size, None);
    }
}
