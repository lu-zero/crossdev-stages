use std::collections::HashMap;
use std::sync::LazyLock;

use camino::{Utf8Path, Utf8PathBuf};
use gentoo_stages::{Arch, Cache, Client};

use crate::error::Result;

/// Per-arch defaults parsed from `config/arch/<arch>.conf`.
/// Files are embedded via `include_str!`; to add a new arch, drop a conf file
/// and append one entry to [`ARCH_CONFIGS`].
struct ArchConfig {
    gentoo_arch: &'static str,
    profile: &'static str,
    kernel_arch: &'static str,
    cflags: &'static str,
    llvm_target: &'static str,
    stage_variant: &'static str,
}

static ARCH_CONFIGS: LazyLock<HashMap<&'static str, ArchConfig>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert("x86_64", parse_arch_conf(include_str!("../config/arch/x86_64.conf")));
    m.insert("aarch64", parse_arch_conf(include_str!("../config/arch/aarch64.conf")));
    m.insert("riscv64", parse_arch_conf(include_str!("../config/arch/riscv64.conf")));
    m
});

fn parse_arch_conf(content: &'static str) -> ArchConfig {
    let map = crate::portage::parse_keyval(content);
    let get = |k: &str| *map.get(k).unwrap_or(&"");
    ArchConfig {
        gentoo_arch: get("GENTOO_ARCH"),
        profile: get("PROFILE"),
        kernel_arch: get("KERNEL_ARCH"),
        cflags: get("CFLAGS"),
        llvm_target: get("LLVM_TARGET"),
        stage_variant: get("STAGE_VARIANT"),
    }
}

fn lookup(arch: &str) -> Result<&'static ArchConfig> {
    ARCH_CONFIGS
        .get(arch)
        .ok_or_else(|| crate::error::Error::UnknownArch(arch.to_string()))
}

/// Map an OS architecture string (e.g. "riscv64") to a `gentoo_stages::Arch`.
/// `gentoo_core::Arch::intern()` takes a Gentoo keyword ("riscv", "amd64", …).
pub fn parse_arch(arch: &str) -> Result<Arch> {
    let keyword = gentoo_arch(arch)?;
    Ok(Arch::intern(keyword))
}

/// Return the Gentoo ARCH keyword for an OS arch string (e.g. "riscv64" → "riscv").
pub fn gentoo_arch(arch: &str) -> Result<&'static str> {
    Ok(lookup(arch)?.gentoo_arch)
}

/// Return the Gentoo profile path for an OS arch string.
pub fn gentoo_profile(arch: &str) -> Result<&'static str> {
    Ok(lookup(arch)?.profile)
}

/// Map an OS architecture string to the Linux kernel `ARCH` value (passed to `make ARCH=…`).
#[allow(dead_code)]
pub fn kernel_arch(arch: &str) -> Result<&'static str> {
    Ok(lookup(arch)?.kernel_arch)
}

/// Default CFLAGS for cross-compilation (board-specific CFLAGS take precedence).
pub fn default_cflags(arch: &str) -> &'static str {
    ARCH_CONFIGS.get(arch).map(|c| c.cflags).unwrap_or("-O3 -pipe")
}

/// Space-separated union of every supported arch's LLVM target name.
/// Used for the host sandbox's make.conf so the bundled LLVM inside
/// `dev-lang/rust` (the crossdev bootstrap compiler) can target any
/// arch we know how to cross-build for.
pub fn all_llvm_targets() -> String {
    let mut targets: Vec<&str> = ARCH_CONFIGS
        .values()
        .map(|c| c.llvm_target)
        .filter(|t| !t.is_empty())
        .collect();
    targets.sort();
    targets.dedup();
    targets.join(" ")
}

/// Map an OS arch to the LLVM target name for `LLVM_TARGETS`.
pub fn llvm_target(arch: &str) -> Option<&'static str> {
    ARCH_CONFIGS
        .get(arch)
        .map(|c| c.llvm_target)
        .filter(|s| !s.is_empty())
}

/// Return the stage3 variant name for `client.get()`.
/// For riscv64 this is "rv64_lp64d-openrc".
pub fn stage_variant(arch: &str) -> &'static str {
    ARCH_CONFIGS
        .get(arch)
        .map(|c| c.stage_variant)
        .unwrap_or("openrc")
}

fn build_client(stages_dir: &Utf8Path, arch: &str, mirror: Option<&str>) -> Result<Client> {
    let gentoo_arch = parse_arch(arch)?;
    let cache = Cache::Path(stages_dir.as_std_path().to_path_buf());
    // The builder's type changes on `.mirror_url()`, so match instead of if-let.
    Ok(match mirror {
        Some(m) => Client::builder()
            .arch(gentoo_arch)
            .cache_dir(cache)
            .mirror_url(m)
            .build()?,
        None => Client::builder().arch(gentoo_arch).cache_dir(cache).build()?,
    })
}

/// Download the stage3 for `arch` into the stages cache directory.
/// Returns the local path to the downloaded tarball.
pub async fn fetch(stages_dir: &Utf8Path, arch: &str, mirror: Option<&str>) -> Result<Utf8PathBuf> {
    let client = build_client(stages_dir, arch, mirror)?;
    let stage = client.get(stage_variant(arch)).await?;
    let path = stage.file_path();
    Utf8PathBuf::try_from(path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()).into())
}

/// List available stage3 images for the given arch.
pub async fn list(stages_dir: &Utf8Path, arch: &str, mirror: Option<&str>) -> Result<Vec<String>> {
    let client = build_client(stages_dir, arch, mirror)?;
    let stages = client.list().await?;
    Ok(stages
        .into_iter()
        .map(|s| {
            format!(
                "{} [{}]",
                s.variant,
                if s.is_cached() { "cached" } else { "remote" }
            )
        })
        .collect())
}
