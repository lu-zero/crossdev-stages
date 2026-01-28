//! Main CLI for crossdev-stages
//!
//! This is the entry point for the crossdev-stages Rust implementation.

use clap::{Arg, Command};
use crossdev_config::PlatformConfig;
use crossdev_stage3::Stage3Fetcher;
use log::{info, LevelFilter};

use std::borrow::Cow;

mod arch;

/// Known architectures with their Gentoo equivalents
#[derive(Debug, Clone, Copy)]
enum KnownArch {
    Aarch64,
    X86_64,
    Riscv64,
    I686,
    Arm,
    Powerpc,
    Powerpc64,
    Mips,
    Mips64,
    S390x,
    Loongarch64,
    Parisc,
    Parisc64,
    Ppc,
    Ppc64,
}

impl KnownArch {
    fn as_str(self) -> &'static str {
        match self {
            KnownArch::Aarch64 => "arm64",
            KnownArch::X86_64 => "amd64",
            KnownArch::Riscv64 => "riscv",
            KnownArch::I686 => "i686",
            KnownArch::Arm => "arm",
            KnownArch::Powerpc => "powerpc",
            KnownArch::Powerpc64 => "powerpc64",
            KnownArch::Mips => "mips",
            KnownArch::Mips64 => "mips64",
            KnownArch::S390x => "s390x",
            KnownArch::Loongarch64 => "loongarch64",
            KnownArch::Parisc => "hppa",
            KnownArch::Parisc64 => "hppa64",
            KnownArch::Ppc => "powerpc",
            KnownArch::Ppc64 => "powerpc64",
        }
    }
}

/// Get the default architecture by converting Rust's std::env::consts::ARCH to Gentoo format
fn get_default_arch() -> Cow<'static, str> {
    match std::env::consts::ARCH {
        "aarch64" => Cow::Borrowed(KnownArch::Aarch64.as_str()),
        "x86_64" => Cow::Borrowed(KnownArch::X86_64.as_str()),
        "riscv64" => Cow::Borrowed(KnownArch::Riscv64.as_str()),
        "i686" => Cow::Borrowed(KnownArch::I686.as_str()),
        "arm" => Cow::Borrowed(KnownArch::Arm.as_str()),
        "powerpc" => Cow::Borrowed(KnownArch::Powerpc.as_str()),
        "powerpc64" => Cow::Borrowed(KnownArch::Powerpc64.as_str()),
        "mips" => Cow::Borrowed(KnownArch::Mips.as_str()),
        "mips64" => Cow::Borrowed(KnownArch::Mips64.as_str()),
        "s390x" => Cow::Borrowed(KnownArch::S390x.as_str()),
        "loongarch64" => Cow::Borrowed(KnownArch::Loongarch64.as_str()),
        "parisc" => Cow::Borrowed(KnownArch::Parisc.as_str()),
        "parisc64" => Cow::Borrowed(KnownArch::Parisc64.as_str()),
        "ppc" => Cow::Borrowed(KnownArch::Ppc.as_str()),
        "ppc64" => Cow::Borrowed(KnownArch::Ppc64.as_str()),
        arch => Cow::Owned(arch.to_string()), // For unknown architectures, use as-is
    }
}

/// Get the default architecture as a static string for clap
fn get_default_arch_static() -> &'static str {
    // Convert to static string for clap's default_value
    match get_default_arch() {
        Cow::Borrowed(s) => s,
        Cow::Owned(s) => Box::leak(s.into_boxed_str()),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::Builder::from_default_env()
        .filter_level(LevelFilter::Info)
        .init();

    // Parse command line arguments
    let matches = Command::new("crossdev-stages")
        .version("0.1.0")
        .about("Gentoo cross-compilation stage builder")
        .subcommand(
            Command::new("fetch")
                .about("Fetch latest stage3 image or list available flavors")
                .arg(
                    Arg::new("arch")
                        .short('a')
                        .long("arch")
                        .value_name("ARCH")
                        .help("Target architecture")
                        .default_value(get_default_arch_static()),
                )
                .arg(
                    Arg::new("flavor")
                        .short('f')
                        .long("flavor")
                        .value_name("FLAVOR")
                        .help("Stage3 flavor (e.g., amd64-openrc)"),
                )
                .arg(
                    Arg::new("mirror")
                        .short('m')
                        .long("mirror")
                        .value_name("URL")
                        .help("Gentoo mirror URL")
                        .default_value("https://distfiles.gentoo.org"),
                )
                .arg(
                    Arg::new("cache")
                        .short('C')
                        .long("cache")
                        .value_name("DIR")
                        .help("Cache directory")
                        .default_value("/tmp/crossdev-stage3-cache"),
                )
                .arg(
                    Arg::new("extract")
                        .short('e')
                        .long("extract")
                        .value_name("DIR")
                        .help("Extract to directory"),
                )
                .arg(
                    Arg::new("list")
                        .short('l')
                        .long("list")
                        .help("List available stage3 flavors instead of fetching")
                        .action(clap::ArgAction::SetTrue),
                ),
        )
        .get_matches();

    match matches.subcommand() {
        Some(("fetch", sub_matches)) => {
            let arch = sub_matches.get_one::<String>("arch").unwrap();
            let flavor = sub_matches.get_one::<String>("flavor");
            let mirror = sub_matches.get_one::<String>("mirror").unwrap();
            let cache_dir = sub_matches.get_one::<String>("cache").unwrap();
            let extract_dir = sub_matches.get_one::<String>("extract");

            // Determine flavor - use architecture-specific defaults
            let flavor = if let Some(f) = flavor {
                f.clone()
            } else {
                // Use the shared function from the arch module
                arch::get_default_flavor(&arch)
            };

            info!("Fetching stage3 for arch={}, flavor={}", arch, flavor);

            // Create minimal configuration for stage3 fetching
            let config = PlatformConfig {
                target: crossdev_config::TargetConfig {
                    arch: arch.clone(),
                    chost: format!("{}-unknown-linux-gnu", arch),
                    flavor: flavor.clone(),
                    keyword: arch.clone(),
                },
                compilation: crossdev_config::CompilationConfig {
                    cflags: "-O2 -pipe".to_string(),
                    gcc_version: "16.0.0".to_string(),
                    profile: "default/linux/amd64/17.1".to_string(),
                },
                repositories: crossdev_config::RepositoryConfig {
                    opensbi_repo: "https://github.com/riscv/opensbi".to_string(),
                    opensbi_tag: "v1.3.1".to_string(),
                    u_boot_repo: "https://github.com/u-boot/u-boot".to_string(),
                    u_boot_tag: "v2023.10".to_string(),
                    firmware_repo: "https://github.com/riscv/firmware".to_string(),
                    firmware_tag: "v1.0".to_string(),
                    kernel_repo: "https://github.com/torvalds/linux".to_string(),
                    kernel_tag: "v6.5".to_string(),
                    bootloader_tag: "v1.0".to_string(),
                },
                packages: crossdev_config::PackageConfig {
                    stage1_file: "stage1-packages.txt".to_string(),
                    additional_file: "additional-packages.txt".to_string(),
                },
                image: crossdev_config::ImageConfig {
                    root_size: "5G".to_string(),
                    boot_size: "500M".to_string(),
                    genimage_config: "genimage.cfg".to_string(),
                },
            };

            // Create stage3 fetcher
            let fetcher = Stage3Fetcher::new(config, cache_dir, mirror);

            // Check if we should list flavors instead of fetching
            if sub_matches.get_flag("list") {
                info!("Listing available stage3 flavors");
                let flavors = fetcher.list_available_flavors()?;

                println!("Available stage3 flavors for {}:", arch);
                println!("===============================");

                if flavors.is_empty() {
                    println!("No stage3 flavors found for architecture: {}", arch);
                    println!("This might mean the architecture is not supported or the mirror is unavailable.");
                    println!("\nTry checking if the architecture exists at:");
                    println!("  {}/releases/", mirror);
                } else {
                    for (i, flavor) in flavors.iter().enumerate() {
                        println!("{}. {}", i + 1, flavor);
                    }
                    println!("\nTotal: {} flavor(s) available", flavors.len());
                    println!("\nTo use a specific flavor, specify it with the --flavor option:");
                    println!(
                        "  cargo run -- fetch --arch {} --flavor {}",
                        arch,
                        flavors.first().unwrap_or(&"unknown".to_string())
                    );
                }
            } else {
                // Fetch latest stage3
                info!("Fetching latest stage3 image...");
                let stage3 = fetcher.fetch_latest()?;

                info!("Latest stage3 image:");
                info!("  Name: {}", stage3.name);
                info!("  URL: {}", stage3.url);
                info!("  Size: {} bytes", stage3.size);
                info!("  Date: {}", stage3.date);
                info!("  Arch: {}", stage3.arch);
                info!("  Flavor: {}", stage3.flavor);

                // Extract if requested
                if let Some(extract_dir) = extract_dir {
                    info!("Extracting to: {}", extract_dir);
                    fetcher.extract_stage3(&stage3, extract_dir)?;
                    info!("Extraction complete!");
                }
            }
        }
        _ => {
            eprintln!("No subcommand specified. Use --help for usage.");
            std::process::exit(1);
        }
    }

    Ok(())
}
