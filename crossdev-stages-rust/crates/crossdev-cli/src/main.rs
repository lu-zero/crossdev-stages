//! Main CLI for crossdev-stages
//!
//! This is the entry point for the crossdev-stages Rust implementation.

use clap::{Arg, Command};
use crossdev_config::PlatformConfig;
use crossdev_stage3::Stage3Fetcher;
use log::{info, error, LevelFilter};
use std::path::PathBuf;

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
            Command::new("fetch-stage3")
                .about("Fetch latest stage3 image")
                .arg(
                    Arg::new("config")
                        .short('c')
                        .long("config")
                        .value_name("FILE")
                        .help("Configuration file")
                        .required(true),
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
        .get_matches();

    match matches.subcommand() {
        Some(("fetch-stage3", sub_matches)) => {
            let config_path = sub_matches.get_one::<String>("config").unwrap();
            let cache_dir = sub_matches.get_one::<String>("cache").unwrap();
            let extract_dir = sub_matches.get_one::<String>("extract");

            // Load configuration
            info!("Loading configuration from: {}", config_path);
            let config = PlatformConfig::load_from_file(config_path)?;
            
            // Create stage3 fetcher
            let fetcher = Stage3Fetcher::new(config, cache_dir);
            
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
        _ => {
            eprintln!("No subcommand specified. Use --help for usage.");
            std::process::exit(1);
        }
    }

    Ok(())
}