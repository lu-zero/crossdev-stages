use camino::{Utf8Path, Utf8PathBuf};
use gentoo_stages::{Arch, Cache, Client};

use crate::error::Result;

/// Map an OS architecture string (e.g. "riscv64") to a `gentoo_stages::Arch`.
/// `gentoo_core::Arch::intern()` takes a Gentoo keyword ("riscv", "amd64", …).
pub fn parse_arch(arch: &str) -> Result<Arch> {
    let keyword = gentoo_arch(arch)?;
    Ok(Arch::intern(keyword))
}

/// Return the Gentoo ARCH keyword for an OS arch string (e.g. "riscv64" → "riscv").
pub fn gentoo_arch(arch: &str) -> Result<&'static str> {
    Ok(match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        a if a.starts_with("riscv") => "riscv",
        a if is_ix86(a) => "x86",
        other => {
            return Err(crate::error::Error::UnknownArch(other.to_string()));
        }
    })
}

/// Return the full CHOST triple for a given arch.
/// x86 32-bit uses the "pc" vendor (Gentoo convention); others use "unknown".
pub fn chost_for_arch(arch: &str) -> Result<String> {
    let vendor = if is_ix86(arch) { "pc" } else { "unknown" };
    Ok(format!("{arch}-{vendor}-linux-gnu"))
}

fn is_ix86(arch: &str) -> bool {
    matches!(arch, "i386" | "i486" | "i586" | "i686")
}

/// Return the Gentoo profile path for an OS arch string.
pub fn gentoo_profile(arch: &str) -> Result<&'static str> {
    Ok(match arch {
        a if a.starts_with("riscv") => "default/linux/riscv/23.0/rv64/lp64d",
        "x86_64" => "default/linux/amd64/23.0",
        "aarch64" => "default/linux/arm64/23.0",
        "i486" | "i586" => "default/linux/x86/23.0/i486",
        "i686" => "default/linux/x86/23.0/i686",
        other => {
            return Err(crate::error::Error::UnknownArch(other.to_string()));
        }
    })
}

/// Map an OS architecture string to the Linux kernel `ARCH` value (passed to `make ARCH=…`).
#[allow(dead_code)]
pub fn kernel_arch(arch: &str) -> Result<&'static str> {
    Ok(match arch {
        "x86_64" => "x86",
        "aarch64" => "arm64",
        a if a.starts_with("arm") => "arm",
        a if a.starts_with("riscv") => "riscv",
        a if a.starts_with("mips") => "mips",
        a if a.starts_with("powerpc") => "powerpc",
        a if a.starts_with("loongarch") => "loongarch",
        a if is_ix86(a) => "x86",
        other => return Err(crate::error::Error::UnknownArch(other.to_string())),
    })
}

/// Default CFLAGS for cross-compilation (board-specific CFLAGS take precedence).
pub fn default_cflags(arch: &str) -> &'static str {
    match arch {
        "x86_64" => "-O3 -march=x86-64 -pipe",
        "aarch64" => "-O3 -pipe",
        "riscv64" => "-O3 -march=rv64gc -pipe",
        _ => "-O3 -pipe",
    }
}

/// Space-separated union of every supported arch's LLVM target name.
/// Used for the host sandbox's make.conf so the bundled LLVM inside
/// `dev-lang/rust` (the crossdev bootstrap compiler) can target any
/// arch we know how to cross-build for.
pub fn all_llvm_targets() -> String {
    let mut targets: Vec<&str> = [
        "x86_64",
        "aarch64",
        "arm",
        "riscv64",
        "mips",
        "loongarch64",
        "powerpc64",
        "sparc64",
    ]
    .into_iter()
    .filter_map(llvm_target)
    .collect();
    targets.sort();
    targets.dedup();
    targets.join(" ")
}

/// Map an OS arch to the LLVM target name for `LLVM_TARGETS`.
pub fn llvm_target(arch: &str) -> Option<&'static str> {
    Some(match arch {
        a if a.starts_with("x86") => "X86",
        a if is_ix86(a) => "X86",
        a if a.starts_with("aarch64") => "AArch64",
        a if a.starts_with("arm") => "ARM",
        a if a.starts_with("riscv") => "RISCV",
        a if a.starts_with("mips") => "Mips",
        a if a.starts_with("loongarch") => "LoongArch",
        a if a.starts_with("powerpc") => "PowerPC",
        a if a.starts_with("sparc") => "Sparc",
        _ => return None,
    })
}

/// Return the stage3 variant name for `client.get()`.
/// For riscv64 this is "rv64_lp64d-openrc".
pub fn stage_variant(arch: &str) -> &'static str {
    match arch {
        a if a.starts_with("riscv") => "rv64_lp64d-openrc",
        "aarch64" => "arm64-openrc",
        "x86_64" => "amd64-openrc",
        "i486" | "i586" => "i486-openrc",
        "i686" => "i686-openrc",
        _ => "openrc",
    }
}

/// Download the stage3 for `arch` into the stages cache directory.
/// Returns the local path to the downloaded tarball.
pub async fn fetch(stages_dir: &Utf8Path, arch: &str, mirror: Option<&str>) -> Result<Utf8PathBuf> {
    let gentoo_arch = parse_arch(arch)?;
    let cache = Cache::Path(stages_dir.as_std_path().to_path_buf());
    let client = match mirror {
        Some(m) => Client::builder()
            .arch(gentoo_arch)
            .cache_dir(cache)
            .mirror_url(m)
            .build()?,
        None => Client::builder()
            .arch(gentoo_arch)
            .cache_dir(cache)
            .build()?,
    };
    let stage = client.get(stage_variant(arch)).await?;
    let path = stage.file_path();
    Utf8PathBuf::try_from(path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()).into())
}

/// List available stage3 images for the given arch.
pub async fn list(stages_dir: &Utf8Path, arch: &str, mirror: Option<&str>) -> Result<Vec<String>> {
    let gentoo_arch = parse_arch(arch)?;
    let cache = Cache::Path(stages_dir.as_std_path().to_path_buf());
    let client = match mirror {
        Some(m) => Client::builder()
            .arch(gentoo_arch)
            .cache_dir(cache)
            .mirror_url(m)
            .build()?,
        None => Client::builder()
            .arch(gentoo_arch)
            .cache_dir(cache)
            .build()?,
    };
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
