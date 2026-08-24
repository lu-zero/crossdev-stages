//! What is in a binary package cache, and whether it is still one set.
//!
//! Gentoo's own guarantee comes from building a binhost as a set: ~19,700
//! packages from one ebuild tree revision on one profile, republished whole.
//! Nothing has to record the compiler because nothing ever mixes.
//!
//! A per-board cache accumulates instead, so the same question -- what
//! produced these files -- has to be answered by writing it down.  Most of it
//! already is: portage writes a `Packages` index whose header carries the same
//! fields Gentoo publishes (PROFILE, ELIBC, CHOST, REPO_REVISIONS).  Only the
//! toolchain is missing, and the store knows that, so this reads both and says
//! when they stop agreeing.
//!
//! Nothing here invalidates a cache.  It is bookkeeping: when the ABI or ISA
//! check fails against a built image, the reason should be one line away
//! rather than an afternoon.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use camino::Utf8Path;

use crate::error::Result;

/// The fields of portage's `Packages` header worth repeating back.
#[derive(Debug, Default)]
pub struct Index {
    pub packages: Option<String>,
    pub profile: Option<String>,
    pub elibc: Option<String>,
    pub chost: Option<String>,
    pub repo_revisions: Option<String>,
}

/// Read the header portage already wrote.  The header ends at the first blank
/// line; everything after it is one stanza per package.
pub fn read_index(binpkg_dir: &Utf8Path) -> Option<Index> {
    let text = std::fs::read_to_string(binpkg_dir.join("Packages")).ok()?;
    let mut index = Index::default();
    for line in text.lines() {
        if line.is_empty() {
            break;
        }
        let Some((key, value)) = line.split_once(": ") else {
            continue;
        };
        let value = value.trim().to_string();
        match key {
            "PACKAGES" => index.packages = Some(value),
            "PROFILE" => index.profile = Some(value),
            "ELIBC" => index.elibc = Some(value),
            "CHOST" => index.chost = Some(value),
            "REPO_REVISIONS" => index.repo_revisions = Some(value),
            _ => {}
        }
    }
    Some(index)
}

/// The cross toolchain, as package name to version.
pub type Toolchain = BTreeMap<String, String>;

/// Read the toolchain out of the store's own portage database, which
/// `setup_crossdev` already keeps at `<store>/.portage-db/<pkg>-<version>`.
pub fn store_toolchain(store_dir: &Utf8Path) -> Toolchain {
    let mut out = Toolchain::new();
    let Ok(entries) = std::fs::read_dir(store_dir.join(".portage-db")) else {
        return out;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if let Some((pkg, version)) = split_pv(&name) {
            out.insert(pkg.to_string(), version.to_string());
        }
    }
    out
}

/// Split `gcc-16.2.9999` into `("gcc", "16.2.9999")`: the version starts at the
/// last hyphen followed by a digit, which is how portage spells it.
fn split_pv(name: &str) -> Option<(&str, &str)> {
    let bytes = name.as_bytes();
    let mut cut = None;
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'-' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
            cut = Some(i);
        }
    }
    let i = cut?;
    Some((&name[..i], &name[i + 1..]))
}

const STAMP: &str = ".build-env";

/// What the stamp says produced this cache.
pub fn read_stamp(binpkg_dir: &Utf8Path) -> Toolchain {
    let mut out = Toolchain::new();
    let Ok(text) = std::fs::read_to_string(binpkg_dir.join(STAMP)) else {
        return out;
    };
    for line in text.lines() {
        if let Some((key, value)) = line.split_once(':') {
            out.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    out
}

/// Record what is producing this cache now.
pub fn write_stamp(binpkg_dir: &Utf8Path, toolchain: &Toolchain) -> Result<()> {
    let mut text = String::from(
        "# What produced the binary packages here.  Portage's own Packages\n\
         # header records the profile, ELIBC, CHOST and tree revision; this\n\
         # records the toolchain, which it does not.\n",
    );
    for (pkg, version) in toolchain {
        let _ = writeln!(text, "{pkg}: {version}");
    }
    std::fs::create_dir_all(binpkg_dir)?;
    std::fs::write(binpkg_dir.join(STAMP), text)?;
    Ok(())
}

/// How the toolchain has moved since this cache was last written to.
/// Empty when nothing changed, or when there was no stamp to compare against.
pub fn drift(old: &Toolchain, new: &Toolchain) -> Vec<String> {
    if old.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (pkg, version) in new {
        match old.get(pkg) {
            Some(previous) if previous != version => {
                out.push(format!("{pkg} {previous} -> {version}"))
            }
            None => out.push(format!("{pkg} {version} (new)")),
            _ => {}
        }
    }
    for pkg in old.keys() {
        if !new.contains_key(pkg) {
            out.push(format!("{pkg} gone"));
        }
    }
    out
}

/// One line at the top of a build: what this cache is, and what has moved.
pub fn report(binpkg_dir: &Utf8Path, store_dir: &Utf8Path) -> Result<()> {
    let now = store_toolchain(store_dir);
    let moved = drift(&read_stamp(binpkg_dir), &now);

    let mut line = format!("binpkgs {}", binpkg_dir.file_name().unwrap_or_default());
    if let Some(index) = read_index(binpkg_dir) {
        if let Some(packages) = &index.packages {
            let _ = write!(line, ": {packages} packages");
        }
        if let Some(profile) = &index.profile {
            let _ = write!(line, ", profile {profile}");
        }
    } else {
        line.push_str(": empty");
    }
    if !moved.is_empty() {
        let _ = write!(line, ", toolchain {}", moved.join(", "));
    }
    println!("{line}");

    if !now.is_empty() {
        write_stamp(binpkg_dir, &now)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{drift, split_pv, Toolchain};

    #[test]
    fn a_package_name_splits_at_the_version() {
        assert_eq!(split_pv("gcc-16.2.9999"), Some(("gcc", "16.2.9999")));
        assert_eq!(split_pv("glibc-2.43-r2"), Some(("glibc", "2.43-r2")));
        assert_eq!(
            split_pv("clang-crossdev-wrappers-21"),
            Some(("clang-crossdev-wrappers", "21"))
        );
        assert_eq!(split_pv("no-version-here"), None);
    }

    #[test]
    fn a_cache_with_no_stamp_reports_no_drift() {
        let mut now = Toolchain::new();
        now.insert("gcc".into(), "16.2.9999".into());
        assert!(drift(&Toolchain::new(), &now).is_empty());
    }

    #[test]
    fn a_moved_toolchain_is_named_in_both_directions() {
        let mut old = Toolchain::new();
        old.insert("gcc".into(), "16.1.0".into());
        old.insert("rust-std".into(), "1.94.1".into());
        let mut new = Toolchain::new();
        new.insert("gcc".into(), "16.2.9999".into());
        new.insert("glibc".into(), "2.43-r2".into());

        let moved = drift(&old, &new);
        assert!(moved.contains(&"gcc 16.1.0 -> 16.2.9999".to_string()));
        assert!(moved.contains(&"glibc 2.43-r2 (new)".to_string()));
        assert!(moved.contains(&"rust-std gone".to_string()));
    }
}
