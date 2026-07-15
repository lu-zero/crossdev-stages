//! Read portage atom lists from text files (`#` comments, blank lines skipped).
//!
//! Line format: `atom [keywords]` — e.g. `sys-boot/syslinux **` or
//! `dev-libs/foo ~amd64`. Keyword overrides are written to
//! `package.accept_keywords/` via [`write_accept_keywords`].
//!
//! Board lists may subtract from the defaults with a `-atom` line
//! (e.g. `-app-editors/vim`); see [`merge`]. Defaults files define the
//! base set, so `-atom` lines in them are rejected by [`read_required`].

use std::io::ErrorKind;

use camino::Utf8Path;

use crate::error::{Error, Result};

#[derive(Debug)]
pub struct Entry {
    pub atom: String,
    /// Optional ACCEPT_KEYWORDS override from the `atom [keywords]` syntax.
    pub keywords: Option<String>,
    /// `-atom` line: remove `atom` from the merged set (board lists only).
    pub remove: bool,
}

/// Read a defaults package list. Errors if the file is missing or contains
/// `-atom` subtraction lines (defaults define the base set; only board
/// lists may subtract).
pub fn read_required(path: &Utf8Path) -> Result<Vec<Entry>> {
    match std::fs::read_to_string(path) {
        Ok(content) => parse_base(&content, path),
        Err(e) if e.kind() == ErrorKind::NotFound => Err(Error::PackageListNotFound(path.into())),
        Err(e) => Err(e.into()),
    }
}

fn parse_base(content: &str, path: &Utf8Path) -> Result<Vec<Entry>> {
    let entries = parse(content);
    if let Some(e) = entries.iter().find(|e| e.remove) {
        return Err(Error::SubtractionInDefaults {
            file: path.into(),
            atom: e.atom.clone(),
        });
    }
    Ok(entries)
}

/// Merge board `overlay` entries into the `base` defaults, in order:
/// additions append, `-atom` entries remove the atom from the set built so
/// far. Subtracting an atom that is not in the set is a no-op with a
/// warning (not an error), so a board list survives a default being
/// dropped from `defaults/`.
pub fn merge(mut base: Vec<Entry>, overlay: Vec<Entry>) -> Vec<Entry> {
    for e in overlay {
        if e.remove {
            let before = base.len();
            base.retain(|b| b.atom != e.atom);
            if base.len() == before {
                tracing::warn!("package list: -{} removes nothing (not in set)", e.atom);
            }
        } else {
            base.push(e);
        }
    }
    base
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
            let atom = parts.next().unwrap_or(line);
            let (atom, remove) = match atom.strip_prefix('-') {
                Some(stripped) => (stripped.to_string(), true),
                None => (atom.to_string(), false),
            };
            let keywords = parts
                .next()
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty());
            Entry {
                atom,
                keywords,
                remove,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atoms_owned(entries: &[Entry]) -> Vec<String> {
        entries.iter().map(|e| e.atom.clone()).collect()
    }

    #[test]
    fn parse_subtraction_line() {
        let entries = parse("-app-editors/vim\nsys-process/htop\n");
        assert_eq!(entries[0].atom, "app-editors/vim");
        assert!(entries[0].remove);
        assert_eq!(entries[1].atom, "sys-process/htop");
        assert!(!entries[1].remove);
    }

    #[test]
    fn merge_subtracts_default() {
        let base = parse("app-editors/vim\nsys-process/htop\n");
        let overlay = parse("-app-editors/vim\nnet-analyzer/nmap\n");
        let merged = merge(base, overlay);
        assert_eq!(
            atoms_owned(&merged),
            vec!["sys-process/htop", "net-analyzer/nmap"]
        );
    }

    #[test]
    fn merge_subtracting_unlisted_atom_is_noop() {
        // Documented behavior: warning, not an error — the merged set is
        // unchanged if the subtracted atom was never in it.
        let base = parse("app-editors/vim\n");
        let overlay = parse("-net-misc/curl\n");
        let merged = merge(base, overlay);
        assert_eq!(atoms_owned(&merged), vec!["app-editors/vim"]);
    }

    #[test]
    fn defaults_reject_subtraction() {
        let path = Utf8Path::new("defaults/target-packages.txt");
        let err = parse_base("net-misc/openssh\n-app-editors/vim\n", path).unwrap_err();
        assert!(matches!(
            err,
            Error::SubtractionInDefaults { ref atom, .. } if atom == "app-editors/vim"
        ));
    }

    #[test]
    fn keywords_still_parse() {
        let entries = parse("sys-boot/syslinux **\n");
        assert_eq!(entries[0].atom, "sys-boot/syslinux");
        assert_eq!(entries[0].keywords.as_deref(), Some("**"));
        assert!(!entries[0].remove);
    }
}
