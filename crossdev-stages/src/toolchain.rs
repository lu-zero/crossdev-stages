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
    sandbox_dir: Utf8PathBuf,
}

impl ToolchainEnv {
    /// Build a toolchain env for an explicit CHOST triple
    /// (e.g. `riscv64-unknown-linux-gnu`) and the sandbox it runs in.
    pub fn new(chost: String, sandbox_dir: Utf8PathBuf) -> Self {
        Self { chost, sandbox_dir }
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

    /// The Cargo/Rust target triple. Equal to the CHOST triple.
    pub fn target_triple(&self) -> &str {
        &self.chost
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

    /// Layer the cross-toolchain environment onto `runner` via
    /// [`SandboxRunner::with_env`], returning the updated runner.
    ///
    /// Sets `CC`, `CXX`, `AR`, `PKG_CONFIG_SYSROOT_DIR`, and
    /// `CARGO_TARGET_<TRIPLE>_LINKER`.
    pub fn apply(&self, runner: SandboxRunner) -> SandboxRunner {
        runner
            .with_env("CC", self.cc())
            .with_env("CXX", self.cxx())
            .with_env("AR", self.ar())
            .with_env("PKG_CONFIG_SYSROOT_DIR", self.pkg_config_sysroot_dir())
            .with_env(self.cargo_linker_key(), self.cc())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn riscv64_toolchain_names() {
        let te = ToolchainEnv::for_arch("riscv64", Utf8PathBuf::from("/sandbox")).unwrap();
        assert_eq!(te.cc(), "riscv64-unknown-linux-gnu-gcc");
        assert_eq!(te.cxx(), "riscv64-unknown-linux-gnu-g++");
        assert_eq!(te.ar(), "riscv64-unknown-linux-gnu-ar");
        assert_eq!(te.target_triple(), "riscv64-unknown-linux-gnu");
        assert_eq!(te.pkg_config_sysroot_dir(), "/usr/riscv64-unknown-linux-gnu");
        assert_eq!(
            te.cargo_linker_key(),
            "CARGO_TARGET_RISCV64_UNKNOWN_LINUX_GNU_LINKER"
        );
    }
}
