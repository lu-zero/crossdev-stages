use camino::{Utf8Path, Utf8PathBuf};

use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::Result;
use crate::stage::chost_for_arch;

/// Cross-toolchain environment for a crossdev-provisioned sandbox.
///
/// Every program name and path this type produces describes **in-sandbox**
/// execution, not the host. The compiler drivers ([`cc`](Self::cc),
/// [`cxx`](Self::cxx), [`ar`](Self::ar)) are the crossdev wrappers on the
/// sandbox `PATH` (`{chost}-gcc`, `{chost}-g++`, `{chost}-ar`), and
/// [`pkg_config_sysroot_dir`](Self::pkg_config_sysroot_dir) is the crossdev
/// prefix `/usr/{chost}` inside the sandbox — a container path, never a host
/// path.
///
/// Construction and every accessor are pure computation over the CHOST
/// triple; nothing here touches the sandbox filesystem. Use
/// [`apply`](Self::apply) to layer these variables onto a [`SandboxRunner`],
/// which is the only place they take effect.
pub struct ToolchainEnv {
    chost: String,
    rust_target: String,
    sandbox_dir: Utf8PathBuf,
}

impl ToolchainEnv {
    /// Build a toolchain env for an explicit CHOST triple
    /// (e.g. `riscv64-unknown-linux-gnu`) and the sandbox it runs in.
    pub fn new(chost: String, sandbox_dir: Utf8PathBuf) -> Self {
        Self {
            rust_target: rust_target_for_chost(&chost),
            chost,
            sandbox_dir,
        }
    }

    /// Build a toolchain env for an OS arch string (e.g. `riscv64`),
    /// deriving the CHOST via [`crate::stage::chost_for_arch`].
    pub fn for_arch(arch: &str, sandbox_dir: Utf8PathBuf) -> Result<Self> {
        Ok(Self::new(chost_for_arch(arch)?, sandbox_dir))
    }

    /// Build a toolchain env from a board's resolved CHOST
    /// ([`BoardConfig::chost`]).
    pub fn for_board(board: &BoardConfig, sandbox_dir: Utf8PathBuf) -> Self {
        Self::new(board.chost(), sandbox_dir)
    }

    /// The CHOST triple this toolchain targets
    /// (e.g. `riscv64-unknown-linux-gnu`).
    pub fn chost(&self) -> &str {
        &self.chost
    }

    /// The sandbox directory this toolchain lives in.
    pub fn sandbox_dir(&self) -> &Utf8Path {
        &self.sandbox_dir
    }

    /// Cross C compiler driver, e.g. `riscv64-unknown-linux-gnu-gcc`.
    pub fn cc(&self) -> String {
        format!("{}-gcc", self.chost)
    }

    /// Cross C++ compiler driver, e.g. `riscv64-unknown-linux-gnu-g++`.
    pub fn cxx(&self) -> String {
        format!("{}-g++", self.chost)
    }

    /// Cross archiver, e.g. `riscv64-unknown-linux-gnu-ar`.
    pub fn ar(&self) -> String {
        format!("{}-ar", self.chost)
    }

    /// The Cargo/Rust target triple.
    ///
    /// **Not** the CHOST.  GCC and rustc name the same machine differently,
    /// and on two of the three architectures in this tree they disagree:
    /// `riscv64-unknown-linux-gnu` is `riscv64gc-unknown-linux-gnu` to rustc,
    /// and `i586-pc-linux-gnu` is `i586-unknown-linux-gnu`.  Handing cargo a
    /// CHOST gets "target may not be installed" if you are lucky, and a
    /// `CARGO_TARGET_*` variable cargo never reads if you are not.
    pub fn target_triple(&self) -> &str {
        &self.rust_target
    }

    /// pkg-config sysroot: the crossdev prefix `/usr/{chost}` **inside the
    /// sandbox**, so pkg-config rewrites `-I`/`-L` paths against the cross
    /// sysroot rather than the host `/usr`.
    pub fn pkg_config_sysroot_dir(&self) -> String {
        format!("/usr/{}", self.chost)
    }

    /// The `CARGO_TARGET_<TRIPLE>_LINKER` variable name for
    /// [`target_triple`](Self::target_triple) (uppercased, non-alphanumerics
    /// mapped to `_`, matching Cargo's env-var convention).
    fn cargo_linker_key(&self) -> String {
        format!(
            "CARGO_TARGET_{}_LINKER",
            self.target_triple()
                .to_uppercase()
                .replace(|c: char| !c.is_ascii_alphanumeric(), "_")
        )
    }

    /// Layer the cross-toolchain environment onto `runner`.
    ///
    /// The bare `CC`/`CXX`/`AR` are what a Makefile or an autotools configure
    /// reads.  The target-suffixed ones are what the `cc` crate reads, and it
    /// prefers them over the bare names -- which matters because it also
    /// compiles build scripts, and those have to run on the machine doing the
    /// building.  `HOST_CC`/`HOST_CXX` are how it is told which compiler that
    /// is.  Without the pair, a build script gets cross-compiled and cargo
    /// tries to execute a foreign binary.
    pub fn apply(&self, runner: SandboxRunner) -> SandboxRunner {
        let target = self.target_triple().to_string();
        runner
            .with_env("CC", self.cc())
            .with_env("CXX", self.cxx())
            .with_env("AR", self.ar())
            .with_env(format!("CC_{target}"), self.cc())
            .with_env(format!("CXX_{target}"), self.cxx())
            .with_env(format!("AR_{target}"), self.ar())
            // Build scripts and proc macros run here, not on the board.
            .with_env("HOST_CC", "gcc")
            .with_env("HOST_CXX", "g++")
            // Without this cargo builds for the host while CC points at the
            // cross compiler, and the two disagree about everything.
            .with_env("CARGO_BUILD_TARGET", &target)
            .with_env(self.cargo_linker_key(), self.cc())
            .with_env("PKG_CONFIG_SYSROOT_DIR", self.pkg_config_sysroot_dir())
            // SYSROOT alone only rewrites the prefixes of what pkg-config
            // already found, and what it finds by default is the host's.
            .with_env(
                "PKG_CONFIG_LIBDIR",
                format!("{}/usr/lib/pkgconfig", self.pkg_config_sysroot_dir()),
            )
            .with_env("PKG_CONFIG_ALLOW_CROSS", "1")
    }
}

/// The Rust target triple for a GCC CHOST.
///
/// rustc names a machine by the ISA it will actually emit; GCC names it by the
/// family and leaves the rest to `-march`.  So `riscv64` has to become
/// `riscv64gc` (rustc ships no plain `riscv64` Linux target), `armv7a` becomes
/// `armv7`, and the vendor field is always `unknown` where GCC says `pc`.
///
/// An architecture with no known mapping keeps its CHOST: wrong, but wrong in
/// the direction that fails loudly at `cargo build` rather than silently
/// producing a variable nothing reads.
fn rust_target_for_chost(chost: &str) -> String {
    let mut parts = chost.splitn(3, '-');
    let arch = parts.next().unwrap_or("");
    // The ABI suffix is the part GCC and rustc do agree on.
    let abi = chost
        .rsplit_once("-linux-")
        .map(|(_, abi)| abi)
        .unwrap_or("gnu");

    let rust_arch = match arch {
        "riscv64" => "riscv64gc",
        "riscv32" => "riscv32gc",
        "armv7a" | "armv7" => "armv7",
        "armv6j" | "armv6" => "arm",
        other => other,
    };
    format!("{rust_arch}-unknown-linux-{abi}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every triple here was checked against `rustc --print target-list`.
    #[test]
    fn a_chost_becomes_the_triple_rustc_actually_knows() {
        for (chost, rust) in [
            ("riscv64-unknown-linux-gnu", "riscv64gc-unknown-linux-gnu"),
            ("riscv32-unknown-linux-musl", "riscv32gc-unknown-linux-musl"),
            ("aarch64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"),
            ("armv7a-unknown-linux-gnueabihf", "armv7-unknown-linux-gnueabihf"),
            // GCC says the vendor is pc here and rustc says unknown.
            ("i586-pc-linux-gnu", "i586-unknown-linux-gnu"),
            ("x86_64-pc-linux-gnu", "x86_64-unknown-linux-gnu"),
        ] {
            assert_eq!(rust_target_for_chost(chost), rust, "{chost}");
        }
    }

    #[test]
    fn the_compiler_prefix_stays_the_chost_not_the_rust_triple() {
        let te = ToolchainEnv::for_arch("riscv64", Utf8PathBuf::from("/sandbox")).unwrap();
        // crossdev installs the wrappers under the CHOST name; rustc's name
        // for the same machine would find nothing on PATH.
        assert!(te.cc().starts_with(te.chost()));
        assert_ne!(te.chost(), te.target_triple());
    }

    #[test]
    fn riscv64_toolchain_names() {
        let te = ToolchainEnv::for_arch("riscv64", Utf8PathBuf::from("/sandbox")).unwrap();
        assert_eq!(te.cc(), "riscv64-unknown-linux-gnu-gcc");
        assert_eq!(te.cxx(), "riscv64-unknown-linux-gnu-g++");
        assert_eq!(te.ar(), "riscv64-unknown-linux-gnu-ar");
        assert_eq!(te.target_triple(), "riscv64gc-unknown-linux-gnu");
        assert_eq!(te.pkg_config_sysroot_dir(), "/usr/riscv64-unknown-linux-gnu");
        assert_eq!(
            te.cargo_linker_key(),
            "CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_GNU_LINKER"
        );
    }
}
