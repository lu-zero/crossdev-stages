//! Read a portage atom list from a text file (`#` comments, blank lines skipped).

use camino::Utf8Path;

use crate::error::{Error, Result};

/// Read a package list, returning one atom per non-empty, non-comment line.
/// Returns an error if the file is missing.
pub fn read_required(path: &Utf8Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(path).map_err(|_| Error::CommandFailed {
        code: 1,
        reason: format!("required package list not found: {path}"),
    })?;
    Ok(parse(&content))
}

/// Read a package list if the file exists, otherwise return an empty list.
pub fn read_optional(path: &Utf8Path) -> Result<Vec<String>> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(parse(&content)),
        Err(_) => Ok(Vec::new()),
    }
}

fn parse(content: &str) -> Vec<String> {
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect()
}
