use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::BTreeMap;

use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::Result;

/// What went into a single image build. Written as `build.lock.toml` in the
/// build dir at pipeline end. Phase 1: observability only (no enforcement).
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
    sources: BTreeMap<String, SourceEntry>,
}

impl ManifestBuilder {
    pub fn new(board: &BoardConfig) -> Self {
        Self {
            started_at: Utc::now(),
            board: board.name.clone(),
            arch: board.arch.clone(),
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
    /// - absent → `kind = missing` (still recorded, so lock 의 shape 은 board 마다 안정적)
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

    /// Gather toolchain CFLAGS by reading the two relevant make.conf files
    /// inside the sandbox.
    fn read_toolchain(&self, runner: &SandboxRunner) -> Result<Toolchain> {
        let chost = format!("{}-unknown-linux-gnu", self.arch);
        let prefix_mk = format!("/usr/{chost}/etc/portage/make.conf");
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

    pub fn write(
        self,
        runner: &SandboxRunner,
        out_path: &Utf8Path,
    ) -> Result<Utf8PathBuf> {
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
        let body = toml::to_string_pretty(&manifest).map_err(|e| {
            crate::error::Error::CommandFailed {
                code: 1,
                reason: format!("toml serialize failed: {e}"),
            }
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
    runner.run_output(&cmd).map(|s| s.trim().to_string()).unwrap_or_default()
}

fn sha256_of(runner: &SandboxRunner, file: &str) -> Option<String> {
    let cmd = format!("[ -f {file} ] && sha256sum {file} | cut -d' ' -f1 || true");
    runner
        .run_output(&cmd)
        .ok()
        .and_then(|s| {
            let s = s.trim();
            if s.is_empty() { None } else { Some(s.to_string()) }
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
