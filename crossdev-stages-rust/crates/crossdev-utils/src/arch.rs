//! Architecture utilities for crossdev-stages
//!
//! This module provides architecture parsing, normalization, and default flavor
//! selection functionality that can be used across all crates.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

/// Supported architectures for crossdev-stages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    /// ARM 32-bit architecture
    Arm,
    /// ARM 64-bit architecture (AArch64)
    AArch64,
    /// x86 32-bit architecture
    X86,
    /// x86 64-bit architecture
    X86_64,
    /// RISC-V 32-bit architecture
    Riscv32,
    /// RISC-V 64-bit architecture
    Riscv64,
    /// PowerPC 32-bit architecture
    Powerpc,
    /// PowerPC 64-bit architecture
    Powerpc64,
}

impl FromStr for Arch {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let arch_str = s.to_lowercase();

        match arch_str.as_str() {
            // ARM variants
            "arm" | "armv7" | "armv7a" | "armv7l" | "armv7hl" => Ok(Arch::Arm),
            // AArch64 variants
            "aarch64" | "arm64" | "armv8" | "armv8a" => Ok(Arch::AArch64),
            // x86 variants
            "x86" | "i386" | "i486" | "i586" | "i686" => Ok(Arch::X86),
            // x86_64 variants
            "x86_64" | "amd64" => Ok(Arch::X86_64),
            // RISC-V 32-bit variants
            "riscv" | "riscv32" => Ok(Arch::Riscv32),
            // RISC-V 64-bit variants
            "riscv64" => Ok(Arch::Riscv64),
            // PowerPC variants
            "ppc" | "powerpc" => Ok(Arch::Powerpc),
            // PowerPC 64-bit variants
            "ppc64" | "powerpc64" => Ok(Arch::Powerpc64),
            _ => Err(format!("Unsupported architecture: {}", s)),
        }
    }
}

impl Arch {
    /// Convert architecture to LLVM target name
    pub fn as_llvm_target(&self) -> &'static str {
        match self {
            Arch::Arm | Arch::AArch64 => "AArch64",
            Arch::X86 | Arch::X86_64 => "X86",
            Arch::Riscv32 | Arch::Riscv64 => "RISCV",
            Arch::Powerpc | Arch::Powerpc64 => "PowerPC",
        }
    }

    /// Convert architecture to Gentoo keyword
    pub fn as_gentoo_keyword(&self) -> &'static str {
        match self {
            Arch::Arm => "arm",
            Arch::AArch64 => "arm64",
            Arch::X86 => "x86",
            Arch::X86_64 => "amd64",
            Arch::Riscv32 | Arch::Riscv64 => "riscv",
            Arch::Powerpc => "ppc",
            Arch::Powerpc64 => "ppc64",
        }
    }

    /// Get the bitness of the architecture
    pub fn bitness(&self) -> u32 {
        match self {
            Arch::Arm | Arch::X86 | Arch::Riscv32 | Arch::Powerpc => 32,
            Arch::AArch64 | Arch::X86_64 | Arch::Riscv64 | Arch::Powerpc64 => 64,
        }
    }
}

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let discriminant = match self {
            Arch::Arm => "arm",
            Arch::AArch64 => "aarch64",
            Arch::X86 => "x86",
            Arch::X86_64 => "x86_64",
            Arch::Riscv32 => "riscv32",
            Arch::Riscv64 => "riscv64",
            Arch::Powerpc => "powerpc",
            Arch::Powerpc64 => "powerpc64",
        };
        write!(f, "{} ({})", discriminant, self.as_gentoo_keyword())
    }
}

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
        "ppc" => "ppc",
        "ppc64" => "ppc64",
        _ => arch,
    };

    // Then handle prefix replacements
    let arch = if arch.starts_with("parisc") {
        arch.replacen("parisc", "hppa", 1)
    } else if arch.starts_with("ppc") && !arch.starts_with("powerpc") {
        // Keep ppc/ppc64 as-is for ACCEPT_KEYWORDS compatibility
        arch.to_string()
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

/// Map Gentoo architecture to LLVM target
///
/// Returns the appropriate LLVM_TARGETS value for a given Gentoo architecture
/// This is used to configure LLVM for cross-compilation
pub fn arch_to_llvm_target(arch: &str) -> String {
    // Extract architecture from full target string (e.g., "riscv64-unknown-linux-gnu" -> "riscv64")
    let arch_part = if arch.contains('-') {
        // Extract the first part before the first hyphen
        arch.split('-').next().unwrap_or(arch)
    } else {
        arch
    };

    // First normalize the architecture
    let normalized_arch = parse_arch(arch_part);

    match normalized_arch.as_str() {
        // RISC-V architectures
        "riscv" | "riscv64" => "RISCV".to_string(),

        // ARM architectures
        "arm" | "arm64" => "AArch64".to_string(),

        // x86 architectures
        "amd64" | "i386" | "i486" | "i586" | "i686" => "X86".to_string(),

        // PowerPC architectures
        "ppc" | "ppc64" | "powerpc" | "powerpc64" => "PowerPC".to_string(),

        // MIPS architectures
        "mips" | "mipsel" | "mips64" | "mips64el" => "MIPS".to_string(),

        // System Z architectures
        "s390" | "s390x" => "SystemZ".to_string(),

        // SPARC architectures
        "sparc" | "sparc64" => "Sparc".to_string(),

        // ARM big-endian
        "armeb" => "ARM".to_string(),

        // Hexagon
        "hexagon" => "Hexagon".to_string(),

        // NVPTX (NVIDIA PTX)
        "nvptx" | "nvptx64" => "NVPTX".to_string(),

        // AMDGPU
        "amdgpu" => "AMDGPU".to_string(),

        // WebAssembly
        "wasm32" | "wasm64" => "WebAssembly".to_string(),

        // AArch64 big-endian
        "aarch64_be" => "AArch64".to_string(),

        // Default case - include common targets for unknown architectures
        _ => "AArch64 RISCV X86".to_string(),
    }
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
        assert_eq!(parse_arch("ppc"), "ppc");
        assert_eq!(parse_arch("ppc64"), "ppc64");
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

    #[test]
    fn test_arch_to_llvm_target() {
        // Test RISC-V
        assert_eq!(arch_to_llvm_target("riscv"), "RISCV");
        assert_eq!(arch_to_llvm_target("riscv64"), "RISCV");
        assert_eq!(arch_to_llvm_target("riscv64-unknown-linux-gnu"), "RISCV");

        // Test ARM
        assert_eq!(arch_to_llvm_target("arm"), "AArch64");
        assert_eq!(arch_to_llvm_target("arm64"), "AArch64");
        assert_eq!(arch_to_llvm_target("aarch64"), "AArch64");
        assert_eq!(arch_to_llvm_target("aarch64-unknown-linux-gnu"), "AArch64");

        // Test x86
        assert_eq!(arch_to_llvm_target("amd64"), "X86");
        assert_eq!(arch_to_llvm_target("x86_64"), "X86");
        assert_eq!(arch_to_llvm_target("i686"), "X86");
        assert_eq!(arch_to_llvm_target("x86_64-pc-linux-gnu"), "X86");

        // Test PowerPC
        assert_eq!(arch_to_llvm_target("ppc"), "PowerPC");
        assert_eq!(arch_to_llvm_target("ppc64"), "PowerPC");
        assert_eq!(arch_to_llvm_target("powerpc-unknown-linux-gnu"), "PowerPC");

        // Test default case
        assert_eq!(arch_to_llvm_target("unknown"), "AArch64 RISCV X86");
        assert_eq!(
            arch_to_llvm_target("unknown-vendor-os"),
            "AArch64 RISCV X86"
        );
    }

    #[test]
    fn test_arch_enum_from_str() {
        // Test ARM variants
        assert_eq!("arm".parse::<Arch>(), Ok(Arch::Arm));
        assert_eq!("armv7".parse::<Arch>(), Ok(Arch::Arm));
        assert_eq!("armv7a".parse::<Arch>(), Ok(Arch::Arm));

        // Test AArch64 variants
        assert_eq!("aarch64".parse::<Arch>(), Ok(Arch::AArch64));
        assert_eq!("arm64".parse::<Arch>(), Ok(Arch::AArch64));
        assert_eq!("armv8".parse::<Arch>(), Ok(Arch::AArch64));

        // Test x86 variants
        assert_eq!("x86".parse::<Arch>(), Ok(Arch::X86));
        assert_eq!("i686".parse::<Arch>(), Ok(Arch::X86));

        // Test x86_64 variants
        assert_eq!("x86_64".parse::<Arch>(), Ok(Arch::X86_64));
        assert_eq!("amd64".parse::<Arch>(), Ok(Arch::X86_64));

        // Test RISC-V variants
        assert_eq!("riscv".parse::<Arch>(), Ok(Arch::Riscv32));
        assert_eq!("riscv32".parse::<Arch>(), Ok(Arch::Riscv32));
        assert_eq!("riscv64".parse::<Arch>(), Ok(Arch::Riscv64));

        // Test PowerPC variants
        assert_eq!("ppc".parse::<Arch>(), Ok(Arch::Powerpc));
        assert_eq!("powerpc".parse::<Arch>(), Ok(Arch::Powerpc));
        assert_eq!("ppc64".parse::<Arch>(), Ok(Arch::Powerpc64));
        assert_eq!("powerpc64".parse::<Arch>(), Ok(Arch::Powerpc64));

        // Test unsupported architecture
        assert!("mips".parse::<Arch>().is_err());
        assert!("sparc".parse::<Arch>().is_err());
    }

    #[test]
    fn test_arch_enum_methods() {
        // Test LLVM targets
        assert_eq!(Arch::Arm.as_llvm_target(), "AArch64");
        assert_eq!(Arch::AArch64.as_llvm_target(), "AArch64");
        assert_eq!(Arch::X86.as_llvm_target(), "X86");
        assert_eq!(Arch::X86_64.as_llvm_target(), "X86");
        assert_eq!(Arch::Riscv32.as_llvm_target(), "RISCV");
        assert_eq!(Arch::Riscv64.as_llvm_target(), "RISCV");
        assert_eq!(Arch::Powerpc.as_llvm_target(), "PowerPC");
        assert_eq!(Arch::Powerpc64.as_llvm_target(), "PowerPC");

        // Test Gentoo keywords
        assert_eq!(Arch::Arm.as_gentoo_keyword(), "arm");
        assert_eq!(Arch::AArch64.as_gentoo_keyword(), "arm64");
        assert_eq!(Arch::X86.as_gentoo_keyword(), "x86");
        assert_eq!(Arch::X86_64.as_gentoo_keyword(), "amd64");
        assert_eq!(Arch::Riscv32.as_gentoo_keyword(), "riscv");
        assert_eq!(Arch::Riscv64.as_gentoo_keyword(), "riscv");
        assert_eq!(Arch::Powerpc.as_gentoo_keyword(), "ppc");
        assert_eq!(Arch::Powerpc64.as_gentoo_keyword(), "ppc64");

        // Test bitness
        assert_eq!(Arch::Arm.bitness(), 32);
        assert_eq!(Arch::AArch64.bitness(), 64);
        assert_eq!(Arch::X86.bitness(), 32);
        assert_eq!(Arch::X86_64.bitness(), 64);
        assert_eq!(Arch::Riscv32.bitness(), 32);
        assert_eq!(Arch::Riscv64.bitness(), 64);
        assert_eq!(Arch::Powerpc.bitness(), 32);
        assert_eq!(Arch::Powerpc64.bitness(), 64);

        // Test Display trait
        assert_eq!(Arch::Arm.to_string(), "arm (arm)");
        assert_eq!(Arch::AArch64.to_string(), "aarch64 (arm64)");
        assert_eq!(Arch::X86.to_string(), "x86 (x86)");
        assert_eq!(Arch::X86_64.to_string(), "x86_64 (amd64)");
    }

    #[test]
    fn test_arch_enum_case_insensitive() {
        assert_eq!("ARM".parse::<Arch>(), Ok(Arch::Arm));
        assert_eq!("AARCH64".parse::<Arch>(), Ok(Arch::AArch64));
        assert_eq!("X86".parse::<Arch>(), Ok(Arch::X86));
        assert_eq!("X86_64".parse::<Arch>(), Ok(Arch::X86_64));
        assert_eq!("RISCV".parse::<Arch>(), Ok(Arch::Riscv32));
        assert_eq!("RISCV64".parse::<Arch>(), Ok(Arch::Riscv64));
        assert_eq!("PPC".parse::<Arch>(), Ok(Arch::Powerpc));
        assert_eq!("PPC64".parse::<Arch>(), Ok(Arch::Powerpc64));
    }
}
