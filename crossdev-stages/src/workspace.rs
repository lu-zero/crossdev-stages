use camino::{Utf8Path, Utf8PathBuf};

use crate::error::{Error, Result};

const CACHE_SUBDIR: &str = "crossdev-stages";
const STAGES: &str = "stages";
const SANDBOXES: &str = "sandboxes";
const TARGETS: &str = "targets";
const BUILDS: &str = "builds";
const SOURCES: &str = "sources";
const LOGS: &str = "logs";
const STORE: &str = "store";
const BINPKGS: &str = "binpkgs";

/// Manages the on-disk cache layout under `~/.cache/crossdev-stages/`:
/// stages, sandboxes, targets, builds, sources, logs, store, binpkgs.
pub struct Workspace {
    base: Utf8PathBuf,
}

impl Workspace {
    /// Open the workspace at the default XDG cache location
    /// (`$XDG_CACHE_HOME/crossdev-stages`, falling back to `~/.cache`).
    pub fn open() -> Result<Self> {
        Ok(Self::at(dirs_next().join(CACHE_SUBDIR)))
    }

    /// Open the workspace rooted at an explicit `base` directory.
    ///
    /// Lets library and CI consumers use a non-XDG cache root (e.g. a
    /// per-job scratch dir) instead of the default `~/.cache` location.
    pub fn at(base: Utf8PathBuf) -> Self {
        Self { base }
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

    /// Content-addressed crossdev prefix store.  Keyed by [`store_key`]
    /// (`<chost>/<cflags-hash>-gcc<spec>`).
    pub fn store_dir(&self) -> Utf8PathBuf {
        self.base.join(STORE)
    }

    /// Shared binpkg cache (`PKGDIR`).  Populated by `FEATURES=buildpkg`
    /// once Phase 3 wires it.
    pub fn binpkgs_dir(&self) -> Utf8PathBuf {
        self.base.join(BINPKGS)
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
            self.store_dir(),
            self.binpkgs_dir(),
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
    ///
    /// Layout is `builds/<board>/<timestamp>/`; we walk one level of
    /// per-board containers and flatten.  A first-level dir that itself
    /// carries a `.board` marker is a legacy flat build (pre-nesting
    /// layout): it is yielded as an opaque leaf and never recursed into —
    /// its children are the build tree (linux/, gen/, …), and yielding
    /// those would let prune/cleanup delete them.
    pub fn list_builds(&self) -> Result<Vec<Utf8PathBuf>> {
        let root = self.builds_dir();
        if !root.exists() {
            return Ok(vec![]);
        }
        let mut all = Vec::new();
        for entry in std::fs::read_dir(&root)? {
            let entry = entry?;
            let dir = match Utf8PathBuf::try_from(entry.path()) {
                Ok(p) if p.is_dir() => p,
                _ => continue,
            };
            all.extend(build_leaves(
                dir,
                |p| p.join(".board").is_file(),
                |p| list_dirs_by_mtime(p).unwrap_or_default(),
            ));
        }
        all.sort_by(|a, b| {
            let m = |p: &Utf8PathBuf| {
                std::fs::metadata(p)
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
            };
            m(b).cmp(&m(a))
        });
        Ok(all)
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

/// Store key for one crossdev prefix: `<chost>/<cflags-hash>-gcc<spec>`.
///
/// The gcc spec is part of the key: two boards with the same chost and
/// CFLAGS but different `BOARD_GCC_VERSION` must not share a prefix --
/// keying on (chost, cflags-hash) alone made them ping-pong full crossdev
/// rebuilds inside one dir while its `.complete` marker stayed set.  The
/// spec is whatever is known host-side before entering the sandbox: the
/// CLI/board version string verbatim when set, otherwise the auto-detected
/// highest installed gcc slot.  Both the cflags-hash and the spec are
/// opaque path segments; nothing may assume their width or format.
pub fn store_key(chost: &str, cflags_hash: &str, gcc_spec: &str) -> Utf8PathBuf {
    Utf8PathBuf::from(chost).join(format!("{cflags_hash}-gcc{gcc_spec}"))
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

/// Classify one first-level entry under `builds/` and return its build
/// leaves.  Pure: filesystem access is injected so the classification is
/// unit-testable.
///
/// - dir carries a `.board` marker → legacy flat build; the dir IS the
///   leaf.  Never descend: its children are the build tree, not builds.
/// - otherwise → per-board container; children with `.board` are leaves.
fn build_leaves(
    dir: Utf8PathBuf,
    has_marker: impl Fn(&Utf8Path) -> bool,
    children: impl Fn(&Utf8Path) -> Vec<Utf8PathBuf>,
) -> Vec<Utf8PathBuf> {
    if has_marker(&dir) {
        return vec![dir];
    }
    children(&dir)
        .into_iter()
        .filter(|c| has_marker(c))
        .collect()
}

/// Read the `.arch` marker file from a sandbox/target directory.
pub fn read_arch(dir: &Utf8Path) -> Option<String> {
    std::fs::read_to_string(dir.join(".arch"))
        .ok()
        .map(|s| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_flat_build_is_an_opaque_leaf() {
        // builds/k1/ carries .board itself (pre-nesting layout); its
        // children are the build tree and must never surface as builds
        // (prune would remove_dir_all them).
        let leaves = build_leaves(
            Utf8PathBuf::from("/builds/k1"),
            |p| p.as_str() == "/builds/k1",
            |_| vec!["/builds/k1/linux".into(), "/builds/k1/gen".into()],
        );
        assert_eq!(leaves, vec![Utf8PathBuf::from("/builds/k1")]);
    }

    #[test]
    fn nested_layout_yields_marked_leaves_only() {
        let marked = ["/builds/k1/20260101T000000Z", "/builds/k1/20260202T000000Z"];
        let leaves = build_leaves(
            Utf8PathBuf::from("/builds/k1"),
            |p| marked.contains(&p.as_str()),
            |_| {
                vec![
                    "/builds/k1/20260101T000000Z".into(),
                    "/builds/k1/20260202T000000Z".into(),
                    "/builds/k1/stray".into(),
                ]
            },
        );
        assert_eq!(
            leaves,
            vec![
                Utf8PathBuf::from("/builds/k1/20260101T000000Z"),
                Utf8PathBuf::from("/builds/k1/20260202T000000Z"),
            ]
        );
    }

    #[test]
    fn empty_container_yields_nothing() {
        let leaves = build_leaves(Utf8PathBuf::from("/builds/k1"), |_| false, |_| vec![]);
        assert!(leaves.is_empty());
    }

    #[test]
    fn store_key_differs_on_gcc_spec() {
        // Same chost + CFLAGS, different BOARD_GCC_VERSION -> distinct dirs.
        let a = store_key("riscv64-unknown-linux-gnu", "0123456789abcdef", "14");
        let b = store_key("riscv64-unknown-linux-gnu", "0123456789abcdef", "15");
        assert_ne!(a, b);
    }

    #[test]
    fn store_key_differs_on_cflags_hash() {
        let a = store_key("riscv64-unknown-linux-gnu", "0123456789abcdef", "15");
        let b = store_key("riscv64-unknown-linux-gnu", "fedcba9876543210", "15");
        assert_ne!(a, b);
    }

    #[test]
    fn store_key_is_stable() {
        assert_eq!(
            store_key("riscv64-unknown-linux-gnu", "0123456789abcdef", "15"),
            Utf8PathBuf::from("riscv64-unknown-linux-gnu/0123456789abcdef-gcc15"),
        );
        // Version-prefix specs key verbatim.
        assert_eq!(
            store_key(
                "aarch64-unknown-linux-gnu",
                "00ff00ff00ff00ff",
                "15.2.1_p20260214"
            ),
            Utf8PathBuf::from("aarch64-unknown-linux-gnu/00ff00ff00ff00ff-gcc15.2.1_p20260214"),
        );
    }
}
