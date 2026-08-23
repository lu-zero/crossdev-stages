use sokgi::{Dialect, FlagSet, Warning};

/// Canonicalize a CFLAGS string and hash it into a short stable id.
///
/// Returns `(canonical, hash)`.  The hash is sokgi's frozen 16-hex FNV-1a
/// digest of the canonical bytes — independent of rustc version, platform
/// and sokgi release, so it is safe as a persistent content-addressed
/// store key.
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

/// Reject CFLAGS that cannot honestly name a store key.
///
/// The hash is what keeps one board's binary packages away from another's, so
/// two flag sets that produce different code must never reduce to the same
/// key, and one flag set must not mean different things on different machines.
/// Two of sokgi's warnings say exactly that has happened, and both are cheap
/// to state and impossible to notice later:
///
///   * `-march=native` asks this machine what it is.  The key would be the
///     same on a machine that answered differently.
///   * `-march` together with `-mcpu`: sokgi 0.2 drops the `-mcpu` when both
///     are given, so two boards differing only in their core would share a
///     key and each other's binaries.
///
/// Returns the offending flag, so the caller can say which board.
pub fn check(cflags: &str) -> Result<(), String> {
    let Ok((_set, warnings)) = FlagSet::parse(cflags, Dialect::C) else {
        // A parse failure falls back to hashing the raw string, which is
        // still a faithful key.  Nothing to refuse.
        return Ok(());
    };
    for warning in &warnings {
        match warning {
            Warning::MachineDependent(flag) => {
                return Err(format!(
                    "{flag} depends on the build machine, so it cannot name a \
                     cache that outlives it"
                ));
            }
            Warning::DroppedByOverride { dropped, by } => {
                return Err(format!(
                    "{dropped} is dropped when {by} is also given, so boards \
                     differing only in {dropped} would share binary packages"
                ));
            }
            _ => {}
        }
    }
    Ok(())
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
    fn a_key_that_would_differ_per_machine_is_refused() {
        assert!(check("-O2 -march=native -pipe").is_err());
    }

    #[test]
    fn flags_that_would_silently_merge_two_boards_are_refused() {
        // sokgi drops -mcpu here, so cortex-a76 and cortex-a53 would collide.
        assert!(check("-O2 -march=armv8-a -mcpu=cortex-a76").is_err());
    }

    #[test]
    fn every_board_in_the_tree_passes() {
        for cflags in [
            "-O3 -march=rv64gcv_zvl256b -pipe",
            "-O3 -march=rv64gcv_zvl128b -pipe",
            "-O3 -march=rv64gcv_zvl512b -pipe",
            "-O3 -mcpu=cortex-a76.cortex-a55+crc+crypto -pipe",
            "-O3 -mcpu=cortex-a55+crc+crypto -pipe",
            "-O2 -march=armv7ve -mtune=cortex-a15.cortex-a7 -mfpu=neon-vfpv4 -mfloat-abi=hard -pipe",
            "-march=pentium-mmx -O2 -pipe",
        ] {
            assert!(check(cflags).is_ok(), "{cflags}");
        }
    }

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
        assert!(h
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
    }
}
