//! Read portage atom lists from text files (`#` comments, blank lines skipped).
//!
//! Line format: `atom [keywords]` — e.g. `sys-boot/syslinux **` or
//! `dev-libs/foo ~amd64`. Keyword overrides are written to
//! `package.accept_keywords/` via [`write_accept_keywords`].

use std::io::ErrorKind;

use camino::Utf8Path;

use crate::error::{Error, Result};

pub struct Entry {
    pub atom: String,
    /// Optional ACCEPT_KEYWORDS override from the `atom [keywords]` syntax.
    pub keywords: Option<String>,
}

/// Read a package list. Returns an error if the file is missing.
pub fn read_required(path: &Utf8Path) -> Result<Vec<Entry>> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(parse(&content)),
        Err(e) if e.kind() == ErrorKind::NotFound => Err(Error::PackageListNotFound(path.into())),
        Err(e) => Err(e.into()),
    }
}

/// Read a package list if the file exists, otherwise return an empty list.
pub fn read_optional(path: &Utf8Path) -> Result<Vec<Entry>> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(parse(&content)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
}

/// Borrow just the atoms, e.g. for an emerge command line.
pub fn atoms(entries: &[Entry]) -> Vec<&str> {
    entries.iter().map(|e| e.atom.as_str()).collect()
}

/// Write `package.accept_keywords/` entries under `portage_dir` (an
/// `etc/portage` directory) for atoms with keyword overrides.
pub fn write_accept_keywords(entries: &[Entry], portage_dir: &Utf8Path) -> Result<()> {
    let dir = portage_dir.join("package.accept_keywords");
    for e in entries {
        if let Some(keywords) = &e.keywords {
            std::fs::create_dir_all(&dir)?;
            let safe_name = e.atom.replace('/', "_");
            std::fs::write(dir.join(safe_name), format!("{} {keywords}\n", e.atom))?;
        }
    }
    Ok(())
}

/// Copy `atom use_flags...` lines from `path` (if present) into
/// `portage_dir/package.use/`, one file per atom.
/// Format: e.g. `sys-boot/grub grub_platforms_pc -grub_platforms_efi-64`.
pub fn write_package_use(path: &Utf8Path, portage_dir: &Utf8Path) -> Result<()> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    let dir = portage_dir.join("package.use");
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        std::fs::create_dir_all(&dir)?;
        let atom = line.split_whitespace().next().unwrap_or(line);
        let safe_name = atom.replace('/', "_");
        std::fs::write(dir.join(safe_name), format!("{line}\n"))?;
    }
    Ok(())
}

fn parse(content: &str) -> Vec<Entry> {
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|line| {
            let mut parts = line.splitn(2, char::is_whitespace);
            let atom = parts.next().unwrap_or(line).to_string();
            let keywords = parts
                .next()
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty());
            Entry { atom, keywords }
        })
        .collect()
}
