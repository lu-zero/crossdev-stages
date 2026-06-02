use camino::{Utf8Path, Utf8PathBuf};

use crate::error::{Error, Result};

const CACHE_SUBDIR: &str = "crossdev-stages";
const STAGES: &str = "stages";
const SANDBOXES: &str = "sandboxes";
const TARGETS: &str = "targets";
const BUILDS: &str = "builds";
const SOURCES: &str = "sources";
const LOGS: &str = "logs";

/// Manages the on-disk cache layout:
/// ~/.cache/crossdev-stages/{stages,sandboxes,targets,builds}/
pub struct Workspace {
    base: Utf8PathBuf,
}

impl Workspace {
    /// Open the workspace at the default XDG cache location.
    pub fn open() -> Result<Self> {
        let base = dirs_next().join(CACHE_SUBDIR);
        Ok(Self { base })
    }

    pub fn base(&self) -> &Utf8Path {
        &self.base
    }

    pub fn stages_dir(&self) -> Utf8PathBuf {
        self.base.join(STAGES)
    }

    pub fn sandboxes_dir(&self) -> Utf8PathBuf {
        self.base.join(SANDBOXES)
    }

    pub fn targets_dir(&self) -> Utf8PathBuf {
        self.base.join(TARGETS)
    }

    pub fn builds_dir(&self) -> Utf8PathBuf {
        self.base.join(BUILDS)
    }

    pub fn sources_dir(&self) -> Utf8PathBuf {
        self.base.join(SOURCES)
    }

    pub fn logs_dir(&self) -> Utf8PathBuf {
        self.base.join(LOGS)
    }

    /// Create all cache subdirectories if they don't exist.
    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [
            self.stages_dir(),
            self.sandboxes_dir(),
            self.targets_dir(),
            self.builds_dir(),
            self.sources_dir(),
            self.logs_dir(),
        ] {
            std::fs::create_dir_all(&dir)?;
        }
        Ok(())
    }

    pub fn sandbox(&self, name: &str) -> Utf8PathBuf {
        self.sandboxes_dir().join(name)
    }

    pub fn target(&self, name: &str) -> Utf8PathBuf {
        self.targets_dir().join(name)
    }

    /// Return all sandbox directories, newest first (by mtime).
    pub fn list_sandboxes(&self) -> Result<Vec<Utf8PathBuf>> {
        list_dirs_by_mtime(&self.sandboxes_dir())
    }

    /// Return all target directories, newest first (by mtime).
    pub fn list_targets(&self) -> Result<Vec<Utf8PathBuf>> {
        list_dirs_by_mtime(&self.targets_dir())
    }

    /// Return all build directories, newest first (by mtime).
    pub fn list_builds(&self) -> Result<Vec<Utf8PathBuf>> {
        list_dirs_by_mtime(&self.builds_dir())
    }

    /// Resolve a sandbox by name or fall back to the most recently modified one.
    pub fn resolve_sandbox(&self, name: Option<&str>) -> Result<Utf8PathBuf> {
        match name {
            Some(n) => {
                let p = self.sandbox(n);
                if p.is_dir() {
                    Ok(p)
                } else {
                    Err(Error::SandboxNotFound(n.to_string()))
                }
            }
            None => self
                .list_sandboxes()?
                .into_iter()
                .next()
                .ok_or_else(|| Error::SandboxNotFound("(none exist)".into())),
        }
    }

    /// Resolve a target by name or fall back to the most recently modified one.
    pub fn resolve_target(&self, name: Option<&str>) -> Result<Utf8PathBuf> {
        match name {
            Some(n) => {
                let p = self.target(n);
                if p.is_dir() {
                    Ok(p)
                } else {
                    Err(Error::TargetNotFound(n.to_string()))
                }
            }
            None => self
                .list_targets()?
                .into_iter()
                .find(|p| p.join("sbin/init").exists())
                .ok_or_else(|| {
                    Error::TargetNotFound("no bootable target (missing /sbin/init)".into())
                }),
        }
    }

    /// Like `resolve_target` but filters targets whose `.arch` marker matches `arch`.
    /// Prevents picking a foreign-arch target (e.g. aarch64) for a board built for
    /// a different arch (e.g. riscv64), which silently produces an unbootable image.
    pub fn resolve_target_for_arch(&self, name: Option<&str>, arch: &str) -> Result<Utf8PathBuf> {
        match name {
            Some(n) => {
                let p = self.target(n);
                if !p.is_dir() {
                    return Err(Error::TargetNotFound(n.to_string()));
                }
                match read_arch(&p) {
                    Some(a) if a == arch => Ok(p),
                    Some(a) => Err(Error::TargetNotFound(format!(
                        "target '{n}' has arch '{a}', expected '{arch}'"
                    ))),
                    None => Err(Error::TargetNotFound(format!(
                        "target '{n}' missing .arch marker"
                    ))),
                }
            }
            None => self
                .list_targets()?
                .into_iter()
                .find(|p| read_arch(p).as_deref() == Some(arch) && p.join("sbin/init").exists())
                .ok_or_else(|| {
                    Error::TargetNotFound(format!(
                        "no bootable target for arch '{arch}' (need /sbin/init and matching .arch)"
                    ))
                }),
        }
    }
}

fn dirs_next() -> Utf8PathBuf {
    // ~/.cache
    if let Ok(cache) = std::env::var("XDG_CACHE_HOME") {
        Utf8PathBuf::from(cache)
    } else {
        let home = std::env::var("HOME").unwrap_or_default();
        Utf8PathBuf::from(home).join(".cache")
    }
}

fn list_dirs_by_mtime(dir: &Utf8Path) -> Result<Vec<Utf8PathBuf>> {
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut entries: Vec<(Utf8PathBuf, std::time::SystemTime)> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = Utf8PathBuf::try_from(e.path()).ok()?;
            if !path.is_dir() {
                return None;
            }
            let mtime = e.metadata().ok()?.modified().ok()?;
            Some((path, mtime))
        })
        .collect();
    entries.sort_by_key(|b| std::cmp::Reverse(b.1));
    Ok(entries.into_iter().map(|(p, _)| p).collect())
}

/// Read the `.arch` marker file from a sandbox/target directory.
pub fn read_arch(dir: &Utf8Path) -> Option<String> {
    std::fs::read_to_string(dir.join(".arch"))
        .ok()
        .map(|s| s.trim().to_string())
}
