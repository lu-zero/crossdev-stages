use std::hash::{Hash, Hasher};

use sokgi::{Dialect, FlagSet};

/// Canonicalize a CFLAGS string and hash it into a short stable id.
///
/// Returns `(canonical, hash12)`. The hash is the first 12 hex chars of a
/// 64-bit FxHash-style fold of the canonical bytes — short enough for path
/// segments, long enough that collisions are not a practical concern within
/// a single workspace.
pub fn canonicalize(cflags: &str) -> (String, String) {
    let canonical = match FlagSet::parse(cflags, Dialect::C) {
        Ok((set, _warnings)) => set.canonical(),
        // On parse failure, fall back to the raw string so the hash still
        // varies with the input. sokgi is permissive (unknown flags emit a
        // warning, not an error), so this branch is rare.
        Err(_) => cflags.trim().to_string(),
    };
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    canonical.hash(&mut hasher);
    let h = hasher.finish();
    (canonical, format!("{h:016x}")[..12].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reordering_yields_same_hash() {
        let (_, a) = canonicalize("-O2 -g -march=cortex-a76+crc");
        let (_, b) = canonicalize("-march=cortex-a76+crc -g -O2");
        assert_eq!(a, b);
    }

    #[test]
    fn different_arch_differs() {
        let (_, a) = canonicalize("-O2 -march=cortex-a55");
        let (_, b) = canonicalize("-O2 -march=cortex-a76");
        assert_ne!(a, b);
    }

    #[test]
    fn last_wins_o_level() {
        let (_, a) = canonicalize("-O3 -O2 -march=cortex-a55");
        let (_, b) = canonicalize("-O2 -march=cortex-a55");
        assert_eq!(a, b);
    }
}
