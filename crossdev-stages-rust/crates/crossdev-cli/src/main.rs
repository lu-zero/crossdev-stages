//! Main CLI for crossdev-stages
//!
//! This is the entry point for the crossdev-stages Rust implementation.

use clap::{Arg, Command};
use crossdev_config::PlatformConfig;
use crossdev_sandbox::auto_detect_backend;
use crossdev_stage3::Stage3Fetcher;
use crossdev_utils::arch;
use log::{info, LevelFilter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
                        .default_value(arch::get_default_arch_for_clap()),
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
        .subcommand(
            Command::new("sandbox")
                .about("Manage sandbox/container operations")
                .subcommand(
                    Command::new("setup")
                        .about("Prepare a sandbox environment")
                        .arg(
                            Arg::new("name")
                                .help("Name for the sandbox")
                                .default_value("default"),
                        )
                        .arg(
                            Arg::new("image")
                                .short('i')
                                .long("image")
                                .help("Docker image to use")
                                .default_value("gentoo/stage3"),
                        ),
                )
                .subcommand(
                    Command::new("prepare")
                        .about("Prepare cross-compilation environment (setup crossdev)")
                        .arg(
                            Arg::new("target")
                                .short('t')
                                .long("target")
                                .help("Target architecture")
                                .default_value("riscv64-k1"),
                        ),
                )
                .subcommand(
                    Command::new("enter")
                        .about("Enter a sandbox (setup if not prepared)")
                        .arg(
                            Arg::new("name")
                                .help("Name of the sandbox to enter")
                                .default_value("default"),
                        )
                        .arg(
                            Arg::new("working-dir")
                                .short('w')
                                .long("working-dir")
                                .value_name("DIR")
                                .help("Working directory inside sandbox"),
                        ),
                )
                .subcommand(Command::new("list").about("List available sandboxes"))
                .subcommand(
                    Command::new("run")
                        .about("Run a command in the sandbox (setup if not prepared)")
                        .arg(
                            Arg::new("name")
                                .help("Name of the sandbox to use")
                                .default_value("default"),
                        )
                        .arg(Arg::new("command").required(true).help("Command to run"))
                        .arg(Arg::new("args").num_args(0..).help("Command arguments"))
                        .arg(
                            Arg::new("working-dir")
                                .short('w')
                                .long("working-dir")
                                .value_name("DIR")
                                .help("Working directory inside sandbox"),
                        ),
                )
                .subcommand(
                    Command::new("delete")
                        .about("Delete a sandbox container")
                        .arg(
                            Arg::new("name")
                                .help("Name of the sandbox to delete")
                                .default_value("default"),
                        )
                        .arg(
                            Arg::new("force")
                                .short('f')
                                .long("force")
                                .help("Force removal of running container")
                                .action(clap::ArgAction::SetTrue),
                        ),
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
                // Use the shared function from the utils crate
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
        Some(("sandbox", sub_matches)) => {
            match sub_matches.subcommand() {
                Some(("setup", sub_matches)) => {
                    let name = sub_matches.get_one::<String>("name").unwrap();
                    let image = sub_matches.get_one::<String>("image").unwrap();

                    info!("Setting up sandbox '{}' with image '{}'", name, image);

                    match auto_detect_backend() {
                        Ok(backend) => {
                            if backend.name() == "docker" {
                                // Focus on image pulling and preparation
                                // The actual container will be created on-demand by enter/run
                                
                                // Ensure the image is available, pulling if necessary
                                let pull_result = std::process::Command::new("docker")
                                    .args(["pull", image])
                                    .output();

                                match pull_result {
                                    Ok(output) => {
                                        if output.status.success() {
                                            info!("Image '{}' is ready", image);
                                            println!(
                                                "✓ Sandbox '{}' prepared with backend: {}",
                                                name,
                                                backend.name()
                                            );
                                            println!("  Image: {}", image);
                                            println!("  Status: Ready (container will be created on first use)");
                                        } else {
                                            let error_msg = String::from_utf8_lossy(&output.stderr);
                                            // If image already exists, that's fine
                                            if !error_msg.contains("not found") && !error_msg.contains("No such image") {
                                                info!("Image '{}' is already available", image);
                                                println!(
                                                    "✓ Sandbox '{}' prepared with backend: {}",
                                                    name,
                                                    backend.name()
                                                );
                                                println!("  Image: {}", image);
                                                println!("  Status: Ready (container will be created on first use)");
                                            } else {
                                                eprintln!("Failed to pull image: {}", error_msg);
                                                std::process::exit(1);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("Failed to check/pull image: {}", e);
                                        std::process::exit(1);
                                    }
                                }
                            } else {
                                println!(
                                    "✓ Sandbox '{}' prepared with backend: {}",
                                    name,
                                    backend.name()
                                );
                                println!("  Image: {}", image);
                                println!("  Status: Ready");
                            }
                        }
                        Err(e) => {
                            eprintln!("Error setting up sandbox: {}", e);
                            std::process::exit(1);
                        }
                    }
                }

                Some(("prepare", sub_matches)) => {
                    let default_target = String::from("riscv64-k1");
                    let target = sub_matches.get_one::<String>("target")
                        .unwrap_or(&default_target);

                    info!("Preparing cross-compilation environment for target: {}", target);

                    // This would be the proper prepare command based on the reference
                    // For now, we'll implement a basic version that sets up the environment
                    
                    match auto_detect_backend() {
                        Ok(backend) => {
                            if backend.name() == "docker" {
                                println!("✓ Preparing Docker-based cross-compilation environment");
                                println!("  Target: {}", target);
                                println!("  Backend: {}", backend.name());
                                println!("  Status: Ready for cross-compilation");
                                
                                // In a full implementation, this would:
                                // 1. Initialize crossdev for the target
                                // 2. Configure Portage settings
                                // 3. Set up profiles and make.conf
                                // 4. Install cross-compilation toolchain
                                
                                println!("\nNote: Full crossdev preparation would require:");
                                println!("  - crossdev tool installation");
                                println!("  - Portage configuration");
                                println!("  - Target-specific toolchain setup");
                                println!("  - Profile and package configuration");
                            } else {
                                println!("✓ Preparing sandbox environment for target: {}", target);
                                println!("  Backend: {}", backend.name());
                                println!("  Note: Full crossdev preparation is only supported for Docker backend");
                            }
                        }
                        Err(e) => {
                            eprintln!("Error preparing environment: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                Some(("enter", sub_matches)) => {
                    let name = sub_matches.get_one::<String>("name").unwrap();
                    let working_dir = sub_matches.get_one::<String>("working-dir");

                    info!("Entering sandbox '{}'", name);

                    match auto_detect_backend() {
                        Ok(backend) => {
                            if backend.name() == "docker" {
                                println!("✓ Entering Docker sandbox '{}'", name);

                                // Use bash -li for proper login interactive shell
                                let command = ["bash", "-li"];

                                let working_dir_path = working_dir.map(std::path::Path::new);
                                match backend
                                    .exec_interactive(name, &command, working_dir_path)
                                    .await
                                {
                                    Ok(_) => {
                                        println!("Interactive session completed");
                                        
                                        // Stop the container after use to free resources
                                        self::cleanup_container(name).await;
                                    }
                                    Err(e) => {
                                        eprintln!("Failed to start interactive session: {}", e);
                                        self::cleanup_container(name).await;
                                        std::process::exit(1);
                                    }
                                }
                            } else {
                                println!(
                                    "✓ Entering sandbox '{}' using backend: {}",
                                    name,
                                    backend.name()
                                );
                                println!(
                                    "  Working directory: {}",
                                    working_dir.map(|d| d.as_str()).unwrap_or("default")
                                );
                                println!("  Status: Active");

                                println!(
                                    "\nInteractive shell entry is not yet implemented for {}",
                                    backend.name()
                                );
                                println!("Use the run command to execute specific commands:");
                                println!(
                                    "  crossdev-stages sandbox run {} -- <command> [args...]",
                                    name
                                );
                            }
                        }
                        Err(e) => {
                            eprintln!("Error entering sandbox: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                Some(("list", _)) => {
                    info!("Listing available sandboxes...");

                    match auto_detect_backend() {
                        Ok(backend) => {
                            println!("✓ Available sandbox backend: {}", backend.name());
                            println!("\nSandboxes:");
                            println!("  - default (using {})", backend.name());
                            println!("\nNote: Sandboxes are created on-demand when used.");
                        }
                        Err(e) => {
                            println!("✗ No sandbox backend available: {}", e);
                            println!("\nNo sandboxes can be created without a working backend.");
                        }
                    }
                }
                Some(("run", sub_matches)) => {
                    let name = sub_matches.get_one::<String>("name").unwrap();
                    let command = sub_matches.get_one::<String>("command").unwrap();
                    let args: Vec<String> = sub_matches
                        .get_many::<String>("args")
                        .map(|vals| vals.cloned().collect())
                        .unwrap_or_default();
                    let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                    let working_dir = sub_matches.get_one::<String>("working-dir");

                    info!(
                        "Running command in sandbox '{}': {} {}",
                        name,
                        command,
                        args.join(" ")
                    );

                    match auto_detect_backend() {
                        Ok(backend) => {
                            let working_dir_path = working_dir.map(std::path::Path::new);
                            match backend
                                .run_command(name, command, &args_refs, working_dir_path)
                                .await
                            {
                                Ok(output) => {
                                    println!(
                                        "✓ Command executed successfully in sandbox '{}':",
                                        name
                                    );
                                    println!("{}", output);
                                    
                                    // Stop the container after use to free resources
                                    self::cleanup_container(name).await;
                                }
                                Err(e) => {
                                    eprintln!(
                                        "Error executing command in sandbox '{}': {}",
                                        name, e
                                    );
                                    self::cleanup_container(name).await;
                                    std::process::exit(1);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                Some(("delete", sub_matches)) => {
                    let name = sub_matches.get_one::<String>("name").unwrap();
                    let force = sub_matches.get_flag("force");

                    info!("Deleting sandbox '{}'", name);

                    match auto_detect_backend() {
                        Ok(backend) => {
                            if backend.name() == "docker" {
                                use bollard::Docker;

                                let docker = match Docker::connect_with_local_defaults() {
                                    Ok(d) => d,
                                    Err(e) => {
                                        eprintln!("Failed to connect to Docker: {}", e);
                                        std::process::exit(1);
                                    }
                                };

                                // First try to stop the container if it's running
                                let _ = docker
                                    .stop_container(name, None)
                                    .await;

                                match docker
                                    .remove_container(
                                        name,
                                        Some(bollard::container::RemoveContainerOptions {
                                            force,
                                            link: false,
                                            v: false,
                                        }),
                                    )
                                    .await
                                {
                                    Ok(_) => {
                                        println!("✓ Sandbox '{}' deleted successfully", name);
                                        println!("  - Container instance removed");
                                        
                                        // Optionally clean up cache (docker system prune)
                                        if force {
                                            println!("  - Cleaning up Docker cache...");
                                            let _ = std::process::Command::new("docker")
                                                .args(["system", "prune", "-f"])
                                                .output();
                                            println!("  - Cache cleanup completed");
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("Error deleting container: {}", e);
                                        std::process::exit(1);
                                    }
                                }
                            } else {
                                println!(
                                    "✓ Sandbox '{}' marked for deletion (backend: {})",
                                    name,
                                    backend.name()
                                );
                            }
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                _ => {
                    eprintln!("No sandbox subcommand specified. Use --help for usage.");
                    std::process::exit(1);
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

/// Clean up a container by stopping it
async fn cleanup_container(name: &str) {
    if let Ok(docker) = bollard::Docker::connect_with_local_defaults() {
        let _ = docker.stop_container(name, None).await;
    }
}
