//! Main CLI for crossdev-stages
//!
//! This is the entry point for the crossdev-stages Rust implementation.

use clap::{Arg, Command};
use crossdev_config::PlatformConfig;
use crossdev_stage3::Stage3Fetcher;
use log::{info, LevelFilter};
use std::io;

/// List available stage3 flavors from Gentoo autobuilds
fn list_available_flavors(arch: &str, mirror: &str) -> Result<(), Box<dyn std::error::Error>> {
    info!("Fetching available stage3 flavors for arch={} from {}", arch, mirror);
    
    let latest_url = format!("{}/releases/{}/autobuilds/latest-stage3.txt", mirror, arch);
    
    info!("Fetching latest stage3 list from: {}", latest_url);
    
    // Use curl to fetch the latest-stage3.txt file
    let output = std::process::Command::new("curl")
        .arg("-s")
        .arg("-f")
        .arg(&latest_url)
        .output()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Box::new(io::Error::new(
            io::ErrorKind::Other,
            format!("Failed to fetch latest-stage3.txt: {}", stderr)
        )));
    }
    
    let content = String::from_utf8_lossy(&output.stdout);
    
    // Parse the latest-stage3.txt content to extract flavors
    let mut flavors = Vec::new();
    
    for line in content.lines() {
        let line = line.trim();
        
        // Skip comments, empty lines, and PGP signature sections
        if line.is_empty() || line.starts_with('#') || line.starts_with("-----") {
            continue;
        }
        
        // Parse stage3 filename from the line
        // Format: timestamp/stage3-arch-flavor-timestamp.tar.xz size
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let filename = parts[0].split('/').last().unwrap_or("");
            
            if filename.starts_with("stage3-") && filename.ends_with(".tar.xz") {
                // Extract flavor from filename: stage3-arch-flavor-timestamp.tar.xz
                // The flavor is everything between "stage3-" and the last "-" before ".tar.xz"
                if let Some(flavor_start) = filename.find("stage3-") {
                    let after_stage3 = &filename[flavor_start + 7..]; // Skip "stage3-"
                    // Remove the .tar.xz extension first
                    let without_ext = after_stage3.replace(".tar.xz", "");
                    // Find the last dash that separates flavor from timestamp
                    if let Some(last_dash) = without_ext.rfind('-') {
                        let flavor = &without_ext[..last_dash];
                        flavors.push(flavor.to_string());
                    }
                }
            }
        }
    }
    
    // Remove duplicates and sort
    flavors.sort();
    flavors.dedup();
    
    println!("Available stage3 flavors for {}:", arch);
    println!("================================");
    
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
        println!("\nTo use a specific flavor, specify it in your platform configuration:");
        println!("  target.flavor = \"{}\"", flavors.first().unwrap_or(&"unknown".to_string()));
        println!("\nYou can also use these flavors with the fetch-stage3 command.");
    }
    
    Ok(())
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
                .about("Fetch latest stage3 image")
                .arg(
                    Arg::new("arch")
                        .short('a')
                        .long("arch")
                        .value_name("ARCH")
                        .help("Target architecture")
                        .default_value("amd64"),
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
                ),
        )
        .subcommand(
            Command::new("list")
                .about("List available stage3 flavors from Gentoo autobuilds")
                .arg(
                    Arg::new("arch")
                        .short('a')
                        .long("arch")
                        .value_name("ARCH")
                        .help("Target architecture (e.g., riscv64, amd64, arm64)")
                        .required(true),
                )
                .arg(
                    Arg::new("mirror")
                        .short('m')
                        .long("mirror")
                        .value_name("URL")
                        .help("Gentoo mirror URL")
                        .default_value("https://distfiles.gentoo.org"),
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

            // Determine flavor - default to {arch}-openrc if not specified
            let flavor = if let Some(f) = flavor {
                f.clone()
            } else {
                format!("{}-openrc", arch)
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
        Some(("list", sub_matches)) => {
            let arch = sub_matches.get_one::<String>("arch").unwrap();
            let mirror = sub_matches.get_one::<String>("mirror").unwrap();
            list_available_flavors(arch, mirror)?;
        }
        _ => {
            eprintln!("No subcommand specified. Use --help for usage.");
            std::process::exit(1);
        }
    }

    Ok(())
}