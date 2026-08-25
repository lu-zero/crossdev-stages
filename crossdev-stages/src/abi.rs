//! Whether the image can actually load its own binaries.
//!
//! `glibc` and `libstdc++` are backward compatible and not forward
//! compatible: a binary built against glibc 2.43 asks the loader for
//! `GLIBC_2.43`, and on an image carrying 2.41 it does not start.  Nothing in
//! a binary package records which libc or compiler produced it -- portage
//! stores CFLAGS, CHOST, USE and the sonames a file needs, but not one
//! symbol version and not the toolchain -- so a package cached from an older
//! or newer prefix installs without complaint and fails at exec.
//!
//! Rather than key the cache on a proxy for that (the gcc version, say, which
//! forces rebuilds nobody needed), read what each binary actually asks for and
//! check the image provides it.  That is exact, it covers glibc, libstdc++,
//! libgcc and every other versioned library at once, and it works on every
//! architecture.

use std::collections::{BTreeMap, BTreeSet};

use crate::container::SandboxRunner;
use crate::error::Result;

/// One unsatisfiable requirement, and who has it.
#[derive(Debug)]
pub struct Unsatisfied {
    pub soname: String,
    /// The version asked for, e.g. `GLIBC_2.43`.  Empty when the library is
    /// missing from the image altogether.
    pub version: String,
    pub count: usize,
    pub examples: Vec<String>,
}

#[derive(Debug)]
pub struct Report {
    pub scanned: usize,
    pub libraries: usize,
    pub unsatisfied: Vec<Unsatisfied>,
}

/// Emit `D`/`N` records for every ELF under `root`:
/// `D<TAB>soname<TAB>version` for a version a library defines, and
/// `N<TAB>path<TAB>soname<TAB>version` for one a file requires.
const SCAN: &str = r#"
find %ROOT% -type f \( -perm -u+x -o -name '*.so' -o -name '*.so.*' \) -print0 \
| xargs -0 -r -n 1 sh -c '
    %READELF% -V "$0" 2>/dev/null | awk -v f="$0" "
        /Version definition section/ { s=\"d\"; next }
        /Version needs section/      { s=\"r\"; next }
        /^Version symbols section/   { s=\"\"; next }
        s == \"d\" && /Flags: BASE/  { if (match(\$0, /Name: [^ ]+/)) { base=substr(\$0, RSTART+6, RLENGTH-6) } ; next }
        s == \"d\" && /Name: /       { if (base != \"\" && match(\$0, /Name: [^ ]+/)) print \"D\t\" base \"\t\" substr(\$0, RSTART+6, RLENGTH-6) ; next }
        s == \"r\" && /File: /       { if (match(\$0, /File: [^ ]+/)) { need=substr(\$0, RSTART+6, RLENGTH-6) } ; next }
        s == \"r\" && /Name: /       { if (need != \"\" && match(\$0, /Name: [^ ]+/)) print \"N\t\" f \"\t\" need \"\t\" substr(\$0, RSTART+6, RLENGTH-6) }
    "
    true'
"#;

/// Check every binary under `root` against the libraries the image ships.
pub fn verify(runner: &SandboxRunner, root: &str, readelf: &str) -> Result<Report> {
    let out = runner.run_output(&SCAN.replace("%ROOT%", root).replace("%READELF%", readelf))?;

    let mut defined: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    // (soname, version) -> files that need it
    let mut needed: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    let mut files: BTreeSet<String> = BTreeSet::new();

    for line in out.lines() {
        let mut parts = line.split('\t');
        match parts.next() {
            Some("D") => {
                let (Some(soname), Some(version)) = (parts.next(), parts.next()) else {
                    continue;
                };
                defined
                    .entry(soname.to_string())
                    .or_default()
                    .insert(version.to_string());
            }
            Some("N") => {
                let (Some(path), Some(soname), Some(version)) =
                    (parts.next(), parts.next(), parts.next())
                else {
                    continue;
                };
                files.insert(path.to_string());
                needed
                    .entry((soname.to_string(), version.to_string()))
                    .or_default()
                    .push(path.to_string());
            }
            _ => {}
        }
    }

    let mut unsatisfied: Vec<Unsatisfied> = needed
        .into_iter()
        .filter(|((soname, version), _)| {
            defined
                .get(soname)
                .is_none_or(|versions| !versions.contains(version))
        })
        .map(|((soname, version), paths)| Unsatisfied {
            // A library the image does not carry at all is a different
            // sentence from one that is simply too old.
            version: if defined.contains_key(&soname) {
                version
            } else {
                String::new()
            },
            soname,
            count: paths.len(),
            examples: paths.into_iter().take(3).collect(),
        })
        .collect();
    unsatisfied.sort_by(|a, b| b.count.cmp(&a.count));

    Ok(Report {
        scanned: files.len(),
        libraries: defined.len(),
        unsatisfied,
    })
}

/// Print a report, and say whether anything in it would fail to start.
pub fn print(report: &Report) -> bool {
    println!(
        "ABI check: {} binaries against {} versioned libraries in the image",
        report.scanned, report.libraries
    );
    if report.scanned == 0 {
        // Not a pass, for the same reason the ISA check says so: a stripped
        // musl tree carries no version records at all, and reporting that as
        // clean would be reporting on nothing.
        println!("    no binary carried a version requirement; nothing was verified");
        return false;
    }
    if report.unsatisfied.is_empty() {
        println!("    every symbol version the image asks for, the image provides");
        return false;
    }
    for u in &report.unsatisfied {
        if u.version.is_empty() {
            println!(
                "    {} binaries need {}, which the image does not ship",
                u.count, u.soname
            );
        } else {
            println!(
                "    {} binaries need {} from {}, which this image's copy does not define",
                u.count, u.version, u.soname
            );
        }
        for path in &u.examples {
            println!("        {path}");
        }
    }
    true
}
