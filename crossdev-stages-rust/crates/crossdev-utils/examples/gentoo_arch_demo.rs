// Example demonstrating the new Arch enum functionality
use crossdev_utils::arch::Arch;

fn main() {
    println!("Testing Arch enum functionality:");

    // Test Arch enum parsing
    println!("\nArch enum parsing:");
    let test_archs = [
        "x86",
        "x86_64",
        "arm",
        "aarch64",
        "riscv32",
        "riscv64",
        "powerpc",
        "powerpc64",
    ];
    for arch_str in test_archs {
        match arch_str.parse::<Arch>() {
            Ok(arch) => {
                println!(
                    "  {} -> {:?} -> LLVM: {} -> Gentoo: {}",
                    arch_str,
                    arch,
                    arch.as_llvm_target(),
                    arch.as_gentoo_keyword()
                );
            }
            Err(e) => {
                println!("  {} -> Error: {}", arch_str, e);
            }
        }
    }

    // Test Arch enum methods
    println!("\nArch enum methods:");
    let archs = [
        Arch::Arm,
        Arch::AArch64,
        Arch::X86,
        Arch::X86_64,
        Arch::Riscv32,
        Arch::Riscv64,
        Arch::Powerpc,
        Arch::Powerpc64,
    ];
    for arch in archs {
        println!(
            "  {:?} -> LLVM: {} -> Gentoo: {} -> Bitness: {}",
            arch,
            arch.as_llvm_target(),
            arch.as_gentoo_keyword(),
            arch.bitness()
        );
    }

    // Test current host architecture
    let host_arch = std::env::consts::ARCH;
    println!("\nCurrent host architecture:");
    println!("  Rust: {}", host_arch);
    match host_arch.parse::<Arch>() {
        Ok(arch) => {
            println!("  Arch: {:?}", arch);
            println!("  LLVM target: {}", arch.as_llvm_target());
            println!("  Gentoo keyword: {}", arch.as_gentoo_keyword());
            println!("  Bitness: {}", arch.bitness());
        }
        Err(e) => {
            println!("  Error parsing host arch: {}", e);
        }
    }
}
