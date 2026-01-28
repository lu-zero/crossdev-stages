//! Architecture utilities for crossdev-stages
//!
//! This module provides architecture parsing, normalization, and default flavor
//! selection functionality that can be used across all crates.

use std::collections::HashMap;

/// Parse architecture name and return normalized version
///
/// Supports common aliases and shorthands:
/// - amd64 -> amd64 (Gentoo uses "amd64" for the directory, not "x86_64")
/// - x86 -> i686
/// - arm64 -> arm64 (Gentoo uses "arm64" for the directory, not "aarch64")
/// - aarch64 -> arm64 (Normalize aarch64 to arm64)
/// - parisc* -> hppa*
/// - ppc/ppc64 -> powerpc*
/// - riscv -> riscv (Gentoo uses "riscv" for the directory)
/// - arm -> arm (Gentoo uses "arm" for the directory)
pub fn parse_arch(arch: &str) -> String {
    // First, handle simple aliases
    let arch = match arch.to_lowercase().as_str() {
        "amd64" => "amd64",  // Gentoo uses "amd64" for the directory
        "x86_64" => "amd64", // Normalize x86_64 to amd64
        "x86" => "i686",
        "arm64" => "arm64",   // Gentoo uses "arm64" for the directory
        "aarch64" => "arm64", // Normalize aarch64 to arm64
        "riscv" => "riscv",   // Gentoo uses "riscv" for the directory
        "arm" => "arm",       // Gentoo uses "arm" for the directory
        "ppc" => "powerpc",
        "ppc64" => "powerpc64",
        _ => arch,
    };

    // Then handle prefix replacements
    let arch = if arch.starts_with("parisc") {
        arch.replacen("parisc", "hppa", 1)
    } else if arch.starts_with("ppc") && !arch.starts_with("powerpc") {
        arch.replacen("ppc", "powerpc", 1)
    } else {
        arch.to_string()
    };

    arch
}

/// Get common architecture aliases mapping
pub fn get_arch_aliases() -> HashMap<&'static str, &'static str> {
    let mut aliases = HashMap::new();
    aliases.insert("amd64", "x86_64");
    aliases.insert("x86", "i686");
    aliases.insert("arm64", "aarch64");
    aliases.insert("riscv", "riscv64");
    aliases.insert("arm", "armv7a");
    aliases.insert("ppc", "powerpc");
    aliases.insert("ppc64", "powerpc64");
    aliases.insert("parisc", "hppa");
    aliases
}

/// Get the default stage3 flavor for a given architecture
///
/// Returns architecture-specific default flavors:
/// - riscv64 → rv64_lp64d-openrc (64-bit RISC-V with LP64D ABI)
/// - riscv → rv32_ilp32d-openrc (32-bit RISC-V with ILP32D ABI)
/// - Other architectures → {arch}-openrc
pub fn get_default_flavor(arch: &str) -> String {
    match arch {
        "riscv64" => "rv64_lp64d-openrc".to_string(),
        "riscv" => "rv32_ilp32d-openrc".to_string(),
        _ => format!("{}-openrc", arch),
    }
}

/// Get the default architecture for clap's default_value
///
/// Returns a static string suitable for clap's default_value parameter
pub fn get_default_arch_for_clap() -> &'static str {
    parse_arch(std::env::consts::ARCH).leak()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_arch() {
        assert_eq!(parse_arch("amd64"), "amd64"); // Gentoo uses "amd64" for directory
        assert_eq!(parse_arch("x86_64"), "amd64"); // Normalize to amd64
        assert_eq!(parse_arch("x86"), "i686");
        assert_eq!(parse_arch("arm64"), "arm64"); // Gentoo uses "arm64" for directory
        assert_eq!(parse_arch("aarch64"), "arm64"); // Normalize to arm64
        assert_eq!(parse_arch("riscv"), "riscv"); // Gentoo uses "riscv" for directory
        assert_eq!(parse_arch("arm"), "arm"); // Gentoo uses "arm" for directory
        assert_eq!(parse_arch("ppc"), "powerpc");
        assert_eq!(parse_arch("ppc64"), "powerpc64");
        assert_eq!(parse_arch("parisc"), "hppa");
        assert_eq!(parse_arch("parisc64"), "hppa64");
    }

    #[test]
    fn test_get_arch_aliases() {
        let aliases = get_arch_aliases();
        assert_eq!(aliases.len(), 8);
        assert_eq!(aliases.get("amd64"), Some(&"x86_64"));
        assert_eq!(aliases.get("arm64"), Some(&"aarch64"));
        assert_eq!(aliases.get("riscv"), Some(&"riscv64"));
    }

    #[test]
    fn test_get_default_flavor() {
        assert_eq!(get_default_flavor("riscv64"), "rv64_lp64d-openrc");
        assert_eq!(get_default_flavor("riscv"), "rv32_ilp32d-openrc");
        assert_eq!(get_default_flavor("amd64"), "amd64-openrc");
        assert_eq!(get_default_flavor("arm64"), "arm64-openrc");
    }
}
