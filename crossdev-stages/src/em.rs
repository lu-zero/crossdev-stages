//! The spec file `em crossdev --setup` reads, generated from a board.

use std::collections::BTreeMap;

use camino::Utf8Path;
use serde::Serialize;

use crate::error::{Error, Result};

/// `make.conf` variables em writes from the spec's own fields.
const SPEC_OWNED: &[&str] = &["CHOST", "CFLAGS", "CXXFLAGS"];

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct Spec<'a> {
    tuple: &'a str,
    profile: &'a str,
    cflags: &'a str,
    extra: Vec<&'a str>,
    toolchain: Toolchains<'a>,
    libc: Libc,
    kernel_headers: Atom,
    system: System,
    package_use: BTreeMap<&'a str, Vec<&'a str>>,
    make_conf: BTreeMap<&'a str, &'a str>,
}

#[derive(Serialize)]
struct Toolchains<'a> {
    gcc: Gcc<'a>,
}

#[derive(Serialize)]
struct Gcc<'a> {
    compiler: &'a str,
    binutils: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct Libc {
    atom: &'static str,
    built_by: &'static str,
}

#[derive(Serialize)]
struct Atom {
    atom: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct System {
    built_by: &'static str,
}

/// A gcc/glibc cross toolchain spec for `chost`, built with `compiler`.
///
/// `package_use` lines are `atom flag…`; `make_conf` drops what the spec's own fields set.
pub fn spec_toml(
    chost: &str,
    profile: &str,
    cflags: &str,
    compiler: &str,
    package_use: &[String],
    make_conf: &[(&str, String)],
) -> Result<String> {
    let package_use = package_use
        .iter()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            Some((words.next()?, words.collect()))
        })
        .collect();
    let make_conf = make_conf
        .iter()
        .filter(|(name, _)| !SPEC_OWNED.contains(name))
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    let spec = Spec {
        tuple: chost,
        profile,
        cflags,
        extra: vec!["sys-devel/clang-crossdev-wrappers"],
        toolchain: Toolchains {
            gcc: Gcc {
                compiler,
                binutils: "sys-devel/binutils",
            },
        },
        libc: Libc {
            atom: "sys-libs/glibc",
            built_by: "gcc",
        },
        kernel_headers: Atom {
            atom: "sys-kernel/linux-headers",
        },
        system: System { built_by: "gcc" },
        package_use,
        make_conf,
    };
    toml::to_string(&spec).map_err(|e| Error::CommandFailed {
        code: 1,
        reason: format!("cannot write the em spec: {e}"),
    })
}

/// The version of `cross-<chost>/gcc` installed in the sandbox at `sandbox_dir`.
pub fn cross_gcc_version(sandbox_dir: &Utf8Path, chost: &str) -> Result<String> {
    let vdb = sandbox_dir.join(format!("var/db/pkg/cross-{chost}"));
    let missing = || Error::CommandFailed {
        code: 1,
        reason: format!("em crossdev installed no cross-{chost}/gcc"),
    };
    let mut versions = Vec::new();
    for entry in std::fs::read_dir(&vdb).map_err(|_| missing())? {
        let name = entry?.file_name();
        if let Some(version) = name.to_str().and_then(|n| n.strip_prefix("gcc-")) {
            versions.push(version.to_string());
        }
    }
    match versions.as_slice() {
        [version] => Ok(version.clone()),
        [] => Err(missing()),
        _ => Err(Error::CommandFailed {
            code: 1,
            reason: format!(
                "several cross-{chost}/gcc installed: {}",
                versions.join(" ")
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_keeps_board_settings_and_leaves_spec_fields_to_em() {
        let text = spec_toml(
            "riscv64-unknown-linux-gnu",
            "default/linux/riscv/23.0/rv64/lp64d",
            "-O3 -march=rv64gcv_zvl256b -pipe",
            "=sys-devel/gcc-16.2*",
            &[
                ">=sys-apps/busybox-1.36.1-r3 -pam static".into(),
                "dev-vcs/git -iconv".into(),
            ],
            &[
                ("MAKEOPTS", "-j17".into()),
                ("CHOST", "riscv64-unknown-linux-gnu".into()),
                ("CFLAGS", "-O2".into()),
                ("CXXFLAGS", "-O2".into()),
            ],
        )
        .unwrap();
        let spec: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(
            spec["cflags"].as_str(),
            Some("-O3 -march=rv64gcv_zvl256b -pipe")
        );
        assert_eq!(
            spec["toolchain"]["gcc"]["compiler"].as_str(),
            Some("=sys-devel/gcc-16.2*")
        );
        let busybox = spec["package-use"][">=sys-apps/busybox-1.36.1-r3"]
            .as_array()
            .unwrap();
        assert_eq!(busybox.len(), 2);
        let make_conf = spec["make-conf"].as_table().unwrap();
        assert_eq!(make_conf.keys().collect::<Vec<_>>(), ["MAKEOPTS"]);
    }

    #[test]
    fn cross_gcc_version_needs_exactly_one() {
        let dir = std::env::temp_dir().join(format!("crossdev-stages-em-{}", std::process::id()));
        let dir = camino::Utf8PathBuf::try_from(dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let chost = "riscv64-unknown-linux-gnu";
        let vdb = dir.join(format!("var/db/pkg/cross-{chost}"));
        assert!(cross_gcc_version(&dir, chost).is_err());
        std::fs::create_dir_all(vdb.join("binutils-2.47")).unwrap();
        std::fs::create_dir_all(vdb.join("gcc-16.2.1_p20261121")).unwrap();
        assert_eq!(cross_gcc_version(&dir, chost).unwrap(), "16.2.1_p20261121");
        std::fs::create_dir_all(vdb.join("gcc-15.2.0")).unwrap();
        assert!(cross_gcc_version(&dir, chost).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
