use sokgi::{Dialect, FlagSet};

/// Canonicalize a CFLAGS string and hash it into a short stable id.
///
/// Returns `(canonical, hash)`.  The hash is sokgi's frozen 16-hex FNV-1a
/// digest of the canonical bytes — independent of rustc version, platform
/// and sokgi release, so it is safe as a persistent content-addressed
/// store key.
///
/// No call sites yet; this is the foundation Phase 3 uses to key the
/// content-addressed crossdev prefix store and per-(chost, cflags-hash)
/// binpkg cache.
#[allow(dead_code)]
pub fn canonicalize(cflags: &str) -> (String, String) {
    match FlagSet::parse(cflags, Dialect::C) {
        Ok((set, _warnings)) => (set.canonical(), set.stable_hash_hex()),
        // sokgi is permissive (unknown flags warn, not error), so this
        // branch is rare.  Hash the trimmed input with sokgi's frozen
        // FNV-1a constants so the fallback keys the same store.
        Err(_) => {
            let canonical = cflags.trim().to_string();
            let hash = fnv1a_hex(canonical.as_bytes());
            (canonical, hash)
        }
    }
}

/// FNV-1a-64 with sokgi's frozen constants, 16 hex chars — matches
/// `FlagSet::stable_hash_hex` so both code paths key the same store.  Only
/// the parse-error fallback needs it, as that path has no `FlagSet` to call
/// `stable_hash_hex` on.
fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
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

    #[test]
    fn hash_is_16_lowercase_hex() {
        let (_, h) = canonicalize("-O2 -march=rv64gc_zba_zbb");
        assert_eq!(h.len(), 16);
        assert!(h.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
    }
}
