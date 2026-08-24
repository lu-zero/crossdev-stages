//! What a board's `-march` promises, against what its binaries actually use.
//!
//! Portage decides a binary package is usable by matching CHOST, KEYWORDS, USE
//! and CPU_FLAGS_X86.  It never matches CFLAGS.  So a package built for one
//! board installs into another that cannot run it, and nothing says so until
//! the hardware hits the first illegal instruction -- which is how the rv32
//! board died on cached `-march=rv32imac` binaries whose compressed opcodes
//! its SoC does not implement.
//!
//! RISC-V only.  It is the arch that both carries the ISA in an ELF attribute
//! (`Tag_RISCV_arch`) and has extensions a board can genuinely lack; x86 and
//! arm64 have no equivalent section to read.

use std::collections::{BTreeMap, BTreeSet};

use camino::Utf8Path;

use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::Result;

/// One distinct ISA found in the scanned tree, and what it costs.
#[derive(Debug)]
pub struct Finding {
    /// Extensions these files use that the board's `-march` does not have.
    pub extra: BTreeSet<String>,
    /// Extensions the board has that these files were not built for.
    pub missing: BTreeSet<String>,
    pub count: usize,
    /// A few paths, enough to go and look.
    pub examples: Vec<String>,
}

#[derive(Debug)]
pub struct Report {
    pub expected: BTreeSet<String>,
    pub scanned: usize,
    pub findings: Vec<Finding>,
}

/// Whether this board has an ISA worth checking.
pub fn applies(board: &BoardConfig) -> bool {
    board.arch.starts_with("riscv")
}

/// The ISA the board's own toolchain produces, asked of the toolchain rather
/// than worked out from a table.
///
/// `-march=rv64gcv_zvl256b` and `-march=rva23u64` both name far more
/// extensions than they spell, and which ones depends on the GCC version.
/// Compiling an empty file and reading the attribute back gives the exact
/// canonical set the same compiler writes into every other object, with no
/// expansion rules to keep in step with the toolchain.
pub fn expected(runner: &SandboxRunner, board: &BoardConfig) -> Result<BTreeSet<String>> {
    let out = runner.run_output(&format!(
        "set -e
         : > /tmp/isa-probe.c
         {cc}gcc {cflags} -c -o /tmp/isa-probe.o /tmp/isa-probe.c
         {cc}readelf -A /tmp/isa-probe.o",
        cc = board.cross_compile,
        cflags = board.effective_cflags(),
    ))?;
    Ok(parse_attributes(&out).unwrap_or_default())
}

/// Read `Tag_RISCV_arch` out of every ELF file under `root`.
///
/// One shell loop rather than one container per file: a rootfs holds thousands
/// of these.
fn scan(runner: &SandboxRunner, root: &str, cross: &str) -> Result<Vec<(String, String)>> {
    let out = runner.run_output(&format!(
        "find {root} -type f \\( -perm -u+x -o -name '*.so' -o -name '*.so.*' \\) -print0 \\
         | xargs -0 -r -n 1 sh -c '
             a=$({cross}readelf -A \"$0\" 2>/dev/null | \\
                 sed -n \"s/.*Tag_RISCV_arch: \\\"\\(.*\\)\\\"/\\1/p\" | head -n 1)
             [ -n \"$a\" ] && printf \"%s\\t%s\\n\" \"$0\" \"$a\"
             true'"
    ))?;
    Ok(out
        .lines()
        .filter_map(|line| {
            let (path, arch) = line.split_once('\t')?;
            Some((path.to_string(), arch.to_string()))
        })
        .collect())
}

/// Compare every binary under `root` against what the board's toolchain emits.
pub fn verify(runner: &SandboxRunner, board: &BoardConfig, root: &str) -> Result<Report> {
    let expected = expected(runner, board)?;
    let found = scan(runner, root, &board.cross_compile)?;

    // Group by ISA string: a rootfs has thousands of files and a handful of
    // distinct answers, and the answer is what matters.
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (path, arch) in &found {
        groups.entry(arch.clone()).or_default().push(path.clone());
    }

    let mut findings = Vec::new();
    for (arch, paths) in groups {
        let observed = parse_arch(&arch);
        let extra: BTreeSet<String> = observed.difference(&expected).cloned().collect();
        let missing: BTreeSet<String> = expected.difference(&observed).cloned().collect();
        if extra.is_empty() && missing.is_empty() {
            continue;
        }
        findings.push(Finding {
            extra,
            missing,
            count: paths.len(),
            examples: paths.iter().take(3).cloned().collect(),
        });
    }
    // Loudest first: the ones that fault, then the biggest.
    findings.sort_by(|a, b| {
        b.extra
            .is_empty()
            .cmp(&a.extra.is_empty())
            .then(b.count.cmp(&a.count))
    });

    Ok(Report {
        expected,
        scanned: found.len(),
        findings,
    })
}

/// Pull the arch string out of `readelf -A` output.
fn parse_attributes(text: &str) -> Option<BTreeSet<String>> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("Tag_RISCV_arch: "))
        .map(|v| parse_arch(v.trim().trim_matches('"')))
}

/// `rv64i2p1_m2p0_zicsr2p0` -> {i, m, zicsr}.
///
/// The XLEN prefix is dropped: a board and its binaries cannot disagree about
/// it without failing far more loudly than this check.
fn parse_arch(arch: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (i, token) in arch.split('_').enumerate() {
        let token = if i == 0 {
            token
                .strip_prefix("rv64")
                .or_else(|| token.strip_prefix("rv32"))
                .or_else(|| token.strip_prefix("rv128"))
                .unwrap_or(token)
        } else {
            token
        };
        let name = strip_version(token);
        if !name.is_empty() {
            out.insert(name.to_string());
        }
    }
    out
}

/// Drop the `<major>p<minor>` an ELF attribute carries and a `-march=` does
/// not: `zicsr2p0` -> `zicsr`, `zvl256b` -> `zvl256b`.
fn strip_version(token: &str) -> &str {
    let bytes = token.as_bytes();
    let mut i = bytes.len();
    while i > 0 && bytes[i - 1].is_ascii_digit() {
        i -= 1;
    }
    if i == bytes.len() || i == 0 || bytes[i - 1] != b'p' {
        return token;
    }
    i -= 1;
    let after_p = i;
    while i > 0 && bytes[i - 1].is_ascii_digit() {
        i -= 1;
    }
    if i == after_p {
        return token;
    }
    &token[..i]
}

/// Print a report, and say whether anything in it is fatal.
pub fn print(report: &Report, root: &Utf8Path) -> bool {
    println!(
        "ISA check: {} binaries under {root}, board ISA {}",
        report.scanned,
        report
            .expected
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
    );
    if report.scanned == 0 {
        // Not a pass.  A rootfs whose binaries carry no section headers -- an
        // OpenWrt image, say -- has no `.riscv.attributes` for readelf to
        // find, so this guard provided nothing.  Saying "every binary matches"
        // after looking at none of them is the shape of bug this check exists
        // to catch.
        println!("    no binary carried an ISA attribute; nothing was verified");
        return false;
    }
    if report.findings.is_empty() {
        println!("    every binary matches the board");
        return false;
    }
    let mut fatal = false;
    for f in &report.findings {
        if f.extra.is_empty() {
            println!(
                "    {} binaries built without {} (slower, but they run)",
                f.count,
                f.missing.iter().cloned().collect::<Vec<_>>().join(" ")
            );
        } else {
            fatal = true;
            println!(
                "    {} binaries use {}, which this board does not have",
                f.count,
                f.extra.iter().cloned().collect::<Vec<_>>().join(" ")
            );
        }
        for path in &f.examples {
            println!("        {path}");
        }
    }
    fatal
}

#[cfg(test)]
mod tests {
    use super::{parse_arch, strip_version};

    #[test]
    fn a_version_suffix_is_not_part_of_the_name() {
        assert_eq!(strip_version("zicsr2p0"), "zicsr");
        assert_eq!(strip_version("i2p1"), "i");
        // Digits that belong to the extension itself stay.
        assert_eq!(strip_version("zvl256b"), "zvl256b");
        assert_eq!(strip_version("zvl256b1p0"), "zvl256b");
        assert_eq!(strip_version("c"), "c");
    }

    #[test]
    fn an_attribute_string_reduces_to_its_extensions() {
        let set = parse_arch("rv64i2p1_m2p0_a2p1_f2p2_d2p2_c2p0_zicsr2p0_zifencei2p0");
        assert!(set.contains("i") && set.contains("c") && set.contains("zifencei"));
        assert!(!set.iter().any(|e| e.starts_with("rv")));
        assert_eq!(set.len(), 8);
    }

    #[test]
    fn a_board_without_compressed_instructions_shows_c_as_extra() {
        let board = parse_arch("rv32i2p1_m2p0_a2p1");
        let binary = parse_arch("rv32i2p1_m2p0_a2p1_c2p0");
        let extra: Vec<_> = binary.difference(&board).cloned().collect();
        assert_eq!(extra, vec!["c".to_string()]);
    }
}
