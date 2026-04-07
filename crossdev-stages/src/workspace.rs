use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

const CACHE_SUBDIR: &str = "crossdev-stages";
const STAGES: &str = "stages";
const SANDBOXES: &str = "sandboxes";
const TARGETS: &str = "targets";
const BUILDS: &str = "builds";
const SYSROOTS: &str = "sysroots";

/// Manages the on-disk cache layout:
/// ~/.cache/crossdev-stages/{stages,sandboxes,targets,builds,sysroots}/
pub struct Workspace {
    base: PathBuf,
}

impl Workspace {
    /// Open the workspace at the default XDG cache location.
    pub fn open() -> Result<Self> {
        let base = dirs_next().join(CACHE_SUBDIR);
        Ok(Self { base })
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    pub fn stages_dir(&self) -> PathBuf {
        self.base.join(STAGES)
    }

    pub fn sandboxes_dir(&self) -> PathBuf {
        self.base.join(SANDBOXES)
    }

    pub fn targets_dir(&self) -> PathBuf {
        self.base.join(TARGETS)
    }

    pub fn builds_dir(&self) -> PathBuf {
        self.base.join(BUILDS)
    }

    pub fn sysroots_dir(&self) -> PathBuf {
        self.base.join(SYSROOTS)
    }

    pub fn sysroot(&self, name: &str) -> PathBuf {
        self.sysroots_dir().join(name)
    }

    /// Create all cache subdirectories if they don't exist.
    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [
            self.stages_dir(),
            self.sandboxes_dir(),
            self.targets_dir(),
            self.builds_dir(),
            self.sysroots_dir(),
        ] {
            std::fs::create_dir_all(&dir)?;
        }
        Ok(())
    }

    pub fn sandbox(&self, name: &str) -> PathBuf {
        self.sandboxes_dir().join(name)
    }

    pub fn target(&self, name: &str) -> PathBuf {
        self.targets_dir().join(name)
    }

    /// Return all sandbox directories, newest first (by mtime).
    pub fn list_sandboxes(&self) -> Result<Vec<PathBuf>> {
        list_dirs_by_mtime(&self.sandboxes_dir())
    }

    /// Return all target directories, newest first (by mtime).
    pub fn list_targets(&self) -> Result<Vec<PathBuf>> {
        list_dirs_by_mtime(&self.targets_dir())
    }

    /// Return all build directories, newest first (by mtime).
    pub fn list_builds(&self) -> Result<Vec<PathBuf>> {
        list_dirs_by_mtime(&self.builds_dir())
    }

    /// Resolve a sandbox by name or fall back to the most recently modified one.
    pub fn resolve_sandbox(&self, name: Option<&str>) -> Result<PathBuf> {
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
    pub fn resolve_target(&self, name: Option<&str>) -> Result<PathBuf> {
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
                .next()
                .ok_or_else(|| Error::TargetNotFound("(none exist)".into())),
        }
    }
}

fn dirs_next() -> PathBuf {
    // ~/.cache
    if let Some(cache) = std::env::var_os("XDG_CACHE_HOME") {
        PathBuf::from(cache)
    } else {
        let home = std::env::var_os("HOME").unwrap_or_default();
        PathBuf::from(home).join(".cache")
    }
}

fn list_dirs_by_mtime(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut entries: Vec<(PathBuf, std::time::SystemTime)> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let mtime = e.metadata().ok()?.modified().ok()?;
            Some((e.path(), mtime))
        })
        .collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1));
    Ok(entries.into_iter().map(|(p, _)| p).collect())
}

/// Read the `.arch` marker file from a sandbox/target directory.
pub fn read_arch(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(".arch")).ok().map(|s| s.trim().to_string())
}
