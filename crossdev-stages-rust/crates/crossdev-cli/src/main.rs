//! Main CLI for crossdev-stages
//!
//! This is the entry point for the crossdev-stages Rust implementation.

use clap::builder::styling::{AnsiColor, Styles};
use clap::{Parser, Subcommand};
use crossdev_sandbox::auto_detect_backend;
use crossdev_stages::{CacheConfig, CacheStrategy, CrossdevCache, Stage3Fetcher};
use crossdev_utils::{arch, arch_to_llvm_target};
use glob::Pattern;
use log::{info, warn, LevelFilter};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::OnceLock;

/// Global cache for the default cache directory path
static DEFAULT_CACHE_DIR: OnceLock<String> = OnceLock::new();

/// Get the default cache directory path using OnceLock for lazy initialization
fn get_default_cache_dir() -> &'static str {
    DEFAULT_CACHE_DIR.get_or_init(|| {
        // Use the existing cache system with Local strategy (XDG-compliant)
        let config = CacheConfig {
            strategy: CacheStrategy::Local,
            ..Default::default()
        };

        match CrossdevCache::new(config) {
            Ok(cache) => cache
                .cache_dir()
                .join("stage3")
                .to_string_lossy()
                .into_owned(),
            Err(e) => {
                warn!(
                    "Failed to initialize cache system: {}, falling back to /tmp",
                    e
                );
                "/tmp/crossdev-stages-cache".to_string()
            }
        }
    })
}

mod crossdev;
mod stage;
use crossdev::CrossdevEnvironment;
use stage::StageManager;

/// Main CLI for crossdev-stages
#[derive(Parser, Debug)]
#[command(
    version = "0.1.0",
    about = "Gentoo cross-compilation stage builder",
    styles = Styles::styled()
        .header(AnsiColor::BrightGreen.on_default().bold())
        .usage(AnsiColor::BrightGreen.on_default().bold())
        .literal(AnsiColor::BrightCyan.on_default().bold())
        .placeholder(AnsiColor::Cyan.on_default())
        .error(AnsiColor::BrightRed.on_default().bold())
        .invalid(AnsiColor::BrightYellow.on_default().bold())
        .valid(AnsiColor::BrightCyan.on_default().bold())
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Manage stage3 images
    #[command(subcommand)]
    Stages(StageCommands),
    /// Manage sandbox/container operations
    #[command(subcommand)]
    Sandbox(SandboxCommands),
}

#[derive(Subcommand, Debug)]
enum StageCommands {
    /// Fetch stage3 images and save to cache
    Fetch(StageFetchArgs),
    /// List available stage3 images in cache
    List(StageListArgs),
    /// Delete stage3 images from cache
    Delete(StageDeleteArgs),
    /// Load a stage3 into a sandbox
    Load(StageLoadArgs),
    /// Save a sandbox as a new stage3
    Save(StageSaveArgs),
    /// Wipe stage files from sandbox
    Wipe(StageWipeArgs),
    /// Update a stage3 with latest packages
    Update(StageUpdateArgs),
    /// List sandbox states and their status
    ListSandboxes,
}

#[derive(clap::Args, Debug)]
struct StageFetchArgs {
    /// Target architecture
    #[arg(short, long, default_value = crossdev_utils::arch::get_default_arch_for_clap())]
    arch: String,

    /// Stage3 flavor (e.g., amd64-openrc)
    #[arg(short, long)]
    flavor: Option<String>,

    /// Gentoo mirror URL
    #[arg(short, long, default_value = "https://distfiles.gentoo.org")]
    mirror: String,

    /// Cache directory
    #[arg(short = 'C', long, default_value = get_default_cache_dir())]
    cache: String,

    /// Extract to directory
    #[arg(short, long)]
    extract: Option<String>,

    /// List available stage3 flavors instead of fetching
    #[arg(short, long)]
    list: bool,
}

#[derive(clap::Args, Debug)]
struct StageListArgs {
    /// Filter by architecture pattern (supports glob patterns)
    #[arg(short, long)]
    arch: Option<String>,

    /// Filter by flavor pattern (supports glob patterns)
    #[arg(short, long)]
    flavor: Option<String>,

    /// Show detailed information including timestamps
    #[arg(short, long)]
    detailed: bool,

    /// Filter by sandbox name to show loaded stages
    #[arg(short = 'S', long)]
    sandbox: Option<String>,

    /// Cache directory
    #[arg(short = 'C', long, default_value = get_default_cache_dir())]
    cache: String,
}

#[derive(clap::Args, Debug)]
struct StageDeleteArgs {
    /// Glob patterns to match stage3 files for deletion
    #[arg(required = true, num_args = 1..)]
    patterns: Vec<String>,

    /// Cache directory
    #[arg(short = 'C', long, default_value = get_default_cache_dir())]
    cache: String,

    /// Dry run - show what would be deleted without actually deleting
    #[arg(short, long)]
    dry_run: bool,

    /// Force deletion without confirmation
    #[arg(short, long)]
    force: bool,
}

#[derive(clap::Args, Debug)]
struct StageLoadArgs {
    /// Name of the sandbox to load the stage into
    #[arg(short = 'S', long, required = true)]
    sandbox: String,

    /// Name of the stage to load (from cache)
    #[arg(required = true)]
    stage: String,

    /// Cache directory
    #[arg(short = 'C', long, default_value = get_default_cache_dir())]
    cache: String,

    /// Force overwrite if stage directory already exists
    #[arg(short, long)]
    force: bool,
}

#[derive(clap::Args, Debug)]
struct StageSaveArgs {
    /// Name of the sandbox to save as a stage
    #[arg(short = 'S', long, required = true)]
    sandbox: String,

    /// Name for the new stage (defaults to stage3-{arch}-{flavor}-cx pattern)
    #[arg(required = true)]
    name: String,

    /// Cache directory to save the new stage
    #[arg(short = 'C', long, default_value = get_default_cache_dir())]
    cache: String,

    /// Architecture specification for the stage name
    #[arg(short = 'a', long)]
    arch: Option<String>,

    /// Flavor specification for the stage name
    #[arg(short = 'v', long)]
    flavor: Option<String>,

    /// Force overwrite if stage already exists
    #[arg(short, long)]
    force: bool,
}

#[derive(clap::Args, Debug)]
struct StageWipeArgs {
    /// Name of the sandbox to wipe stage files from
    #[arg(short = 'S', long, required = true)]
    sandbox: String,

    /// Force deletion without confirmation
    #[arg(short, long)]
    force: bool,
}

#[derive(clap::Args, Debug)]
struct StageUpdateArgs {
    /// Name of the sandbox containing the stage to update
    #[arg(short = 'S', long, required = true)]
    sandbox: String,

    /// Stage directory path (relative to sandbox or absolute path)
    #[arg(short, long)]
    stage_dir: Option<String>,

    /// Update ldconfig cache after updating packages
    #[arg(short, long)]
    ldconfig: bool,

    /// Force update even if stage appears corrupted
    #[arg(short, long)]
    force: bool,
}

#[derive(Subcommand, Debug)]
enum SandboxCommands {
    /// Prepare a sandbox environment
    Setup(SandboxSetupArgs),
    /// Prepare cross-compilation environment (setup crossdev)
    Prepare(SandboxPrepareArgs),
    /// Enter a sandbox (setup if not prepared)
    Enter(SandboxEnterArgs),
    /// List available sandboxes
    List,
    /// Run a command in the sandbox (setup if not prepared)
    Run(SandboxRunArgs),
    /// Delete a sandbox container
    Delete(SandboxDeleteArgs),
}

#[derive(clap::Args, Debug)]
struct SandboxSetupArgs {
    /// Name for the sandbox
    #[arg(default_value = "default")]
    name: String,

    /// Docker image to use
    #[arg(short, long, default_value = "gentoo/stage3")]
    image: String,
}

#[derive(clap::Args, Debug)]
struct SandboxPrepareArgs {
    /// Target architecture
    #[arg(short, long, default_value = "riscv64-k1")]
    target: String,
}

#[derive(clap::Args, Debug)]
struct SandboxEnterArgs {
    /// Name of the sandbox to enter
    #[arg(default_value = "default")]
    name: String,

    /// Working directory inside sandbox
    #[arg(short, long)]
    working_dir: Option<String>,
}

#[derive(clap::Args, Debug)]
struct SandboxRunArgs {
    /// Command to run
    command: String,

    /// Command arguments
    #[arg(num_args = 0..)]
    args: Vec<String>,

    /// Name of the sandbox to use
    #[arg(short, long, default_value = "default")]
    name: String,

    /// Working directory inside sandbox
    #[arg(short, long)]
    working_dir: Option<String>,
}

#[derive(clap::Args, Debug)]
struct SandboxDeleteArgs {
    /// Name of the sandbox to delete
    #[arg(default_value = "default")]
    name: String,

    /// Force removal of running container
    #[arg(short, long)]
    force: bool,
}

/// Clean up a container by stopping it
/// This is called when:
/// - Interactive session completes normally
/// - User presses Ctrl+C (SIGINT)
/// - Any error occurs during interactive session
async fn cleanup_container(name: &str) {
    info!("Stopping container '{}' to free resources", name);
    if let Ok(docker) = bollard::Docker::connect_with_local_defaults() {
        match docker.stop_container(name, None).await {
            Ok(_) => info!("✓ Container '{}' stopped successfully", name),
            Err(e) => warn!("Warning: Failed to stop container '{}': {}", name, e),
        }
    } else {
        warn!(
            "Warning: Docker connection failed during cleanup for container '{}'",
            name
        );
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::Builder::from_default_env()
        .filter_level(LevelFilter::Info)
        .init();

    // Parse command line arguments using derive
    let cli = Cli::parse();

    match cli.command {
        Commands::Stages(stage_cmd) => {
            match stage_cmd {
                StageCommands::Fetch(args) => {
                    // Handle stage fetch (similar to existing Fetch command but saving to cache)
                    let arch = args.arch;
                    let flavor = args.flavor;
                    let mirror = args.mirror;
                    let cache_dir = args.cache;
                    let extract_dir = args.extract;

                    // Determine flavor - use architecture-specific defaults
                    let flavor = if let Some(f) = flavor {
                        f
                    } else {
                        // Use the shared function from the utils crate
                        arch::get_default_flavor(&arch)
                    };

                    info!("Fetching stage3 for arch={}, flavor={}", arch, flavor);

                    // Create target configuration for stage3 fetching
                    let target_config = crossdev_config::TargetConfig {
                        arch: arch.parse()?,
                        flavor: flavor.clone(),
                    };

                    // Create stage3 fetcher using simplified constructor
                    let fetcher = Stage3Fetcher::new_for_fetch(target_config, &cache_dir, &mirror);

                    // Check if we should list flavors instead of fetching
                    if args.list {
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
                            println!(
                                "\nTo use a specific flavor, specify it with the --flavor option:"
                            );
                            println!(
                                "  {} stages fetch --arch {} --flavor {}",
                                std::env::args()
                                    .next()
                                    .unwrap_or_else(|| "crossdev-stages".to_string()),
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
                StageCommands::List(args) => {
                    // Handle stage list with glob pattern support
                    handle_stage_list(args).await?;
                }
                StageCommands::Delete(args) => {
                    // Handle stage delete with glob pattern support
                    handle_stage_delete(args).await?;
                }
                StageCommands::Load(args) => {
                    // Handle stage load
                    handle_stage_load(args).await?;
                }
                StageCommands::Save(args) => {
                    // Handle stage save
                    handle_stage_save(args).await?;
                }
                StageCommands::Wipe(args) => {
                    // Handle stage wipe
                    handle_stage_wipe(args).await?;
                }
                StageCommands::Update(args) => {
                    // Handle stage update
                    handle_stage_update(args).await?;
                }
                StageCommands::ListSandboxes => {
                    // Handle sandbox listing
                    handle_list_sandboxes().await?;
                }
            }
        }
        Commands::Sandbox(sandbox_cmd) => {
            match sandbox_cmd {
                SandboxCommands::Setup(args) => {
                    let name = args.name;
                    let image = args.image;

                    info!("Setting up sandbox '{}' with image '{}'", name, image);

                    match auto_detect_backend() {
                        Ok(backend) => {
                            if backend.name() == "docker" {
                                // Ensure the image is available, pulling if necessary
                                let pull_result = std::process::Command::new("docker")
                                    .args(["pull", &image])
                                    .output();

                                match pull_result {
                                    Ok(output) => {
                                        if output.status.success() {
                                            info!("Image '{}' is ready", image);
                                        } else {
                                            let error_msg = String::from_utf8_lossy(&output.stderr);
                                            // If image already exists, that's fine
                                            if !error_msg.contains("not found")
                                                && !error_msg.contains("No such image")
                                            {
                                                info!("Image '{}' is already available", image);
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

                                // Set up basic Portage configuration in the container
                                info!("Setting up basic Portage environment");

                                // Ensure container is ready by running a simple command
                                let _ = backend
                                    .run_command(&name, "echo", &["Container ready"], None)
                                    .await;

                                // Set ACCEPT_KEYWORDS based on host architecture (setup is always for the host)
                                let host_arch = std::env::consts::ARCH;
                                let gentoo_arch = crossdev_utils::arch::parse_arch(host_arch);

                                // ACCEPT_KEYWORDS is now simply ~ + gentoo_arch
                                // Since arch parsing returns the correct Gentoo architecture names
                                let accept_keyword = format!("~{}", gentoo_arch);

                                info!("Detected host architecture: {} (Gentoo: {}) -> ACCEPT_KEYWORDS={}",
                                    host_arch, gentoo_arch, accept_keyword);
                                info!("Preserving existing make.conf content and updating ACCEPT_KEYWORDS");

                                let accept_keywords_result = backend.run_command(
                                    &name,
                                    "sh",
                                    &["-c", &format!("if [ -f /etc/portage/make.conf ]; then sed -i '/^ACCEPT_KEYWORDS=/d' /etc/portage/make.conf; fi && echo 'ACCEPT_KEYWORDS={}' >> /etc/portage/make.conf", accept_keyword)],
                                    None
                                ).await;

                                match accept_keywords_result {
                                    Ok(_) => {
                                        info!("✓ ACCEPT_KEYWORDS configured to {}", accept_keyword);
                                        info!("  This allows testing/unstable packages for the detected architecture");
                                    }
                                    Err(e) => {
                                        eprintln!("Error: Failed to set ACCEPT_KEYWORDS: {}", e);
                                        std::process::exit(1);
                                    }
                                }

                                // Set MAKEOPTS for parallel builds (adjust based on available CPU cores)
                                // Set MAKEOPTS and EMERGE_DEFAULT_OPTS for optimal performance
                                info!("Configuring MAKEOPTS and EMERGE_DEFAULT_OPTS for optimal performance");
                                info!("  MAKEOPTS will use all available CPU cores with proper load averaging");
                                info!("  EMERGE_DEFAULT_OPTS will enable parallel package installation");

                                // Use default values for basic setup (auto-detect CPU cores)
                                let makeopts = "-j$(nproc) --load-average=$(nproc)";
                                let emerge_opts =
                                    "--jobs=$(nproc) --load-average=$(nproc) --quiet-build y";

                                info!("  Using MAKEOPTS: {}", makeopts);
                                info!("  Using EMERGE_DEFAULT_OPTS: {}", emerge_opts);

                                let makeopts_result = backend.run_command(
                                    &name,
                                    "sh",
                                    &["-c", &format!("echo 'MAKEOPTS=\"{}\"' >> /etc/portage/make.conf && echo 'EMERGE_DEFAULT_OPTS=\"{}\"' >> /etc/portage/make.conf", makeopts, emerge_opts)],
                                    None
                                ).await;

                                match makeopts_result {
                                    Ok(_) => {
                                        info!("✓ MAKEOPTS and EMERGE_DEFAULT_OPTS configured");
                                        info!("  Configuration:");
                                        info!("    - MAKEOPTS=\"{}\"", makeopts);
                                        info!("    - EMERGE_DEFAULT_OPTS=\"{}\"", emerge_opts);
                                    }
                                    Err(e) => {
                                        eprintln!("Warning: Failed to set MAKEOPTS/EMERGE_DEFAULT_OPTS: {}", e);
                                        eprintln!("  Build performance may be suboptimal");
                                    }
                                }

                                // Set up package.use for u-boot-tools with python USE flag
                                info!("Setting up package.use configurations...");
                                let uboot_use_result = backend.run_command(
                                    "default",
                                    "sh",
                                    &["-c", "mkdir -p /etc/portage/package.use && echo 'sys-apps/dtc python' > /etc/portage/package.use/u-boot-tools"],
                                    None
                                ).await;

                                match uboot_use_result {
                                    Ok(_) => info!("✓ package.use/u-boot-tools configured with python USE flag"),
                                    Err(e) => {
                                        eprintln!("Error: Failed to configure u-boot-tools USE flags: {}", e);
                                        std::process::exit(1);
                                    }
                                }

                                // Run emerge --sync to update package database
                                info!("Running emerge --sync to update package database...");
                                let sync_result = backend
                                    .run_command("default", "emerge", &["--sync"], None)
                                    .await;

                                match sync_result {
                                    Ok(_) => info!("✓ Package database synchronized"),
                                    Err(e) => {
                                        eprintln!("Error: Failed to sync package database: {}", e);
                                        std::process::exit(1);
                                    }
                                }

                                // Install essential packages for cross-compilation
                                // Following proper Portage setup order:
                                // 1. ACCEPT_KEYWORDS configured ✓
                                // 2. package.use configured ✓
                                // 3. emerge --sync completed ✓
                                // 4. Install packages (current step)
                                // 5. Repository setup (next step)
                                // Note: We should also consider setting MAKEOPTS in make.conf for optimal build performance
                                info!("Installing cross-compilation prerequisites...");

                                // Install all required dependencies from README.md in a single emerge call
                                // Note: We may want to cache these packages later for faster setup
                                let packages = [
                                    // Needed to build all the stages
                                    "sys-devel/crossdev",
                                    "sys-apps/merge-usr",
                                    "dev-vcs/git",
                                    // Needed to build the bootloader and kernel
                                    "dev-embedded/u-boot-tools",
                                    "sys-apps/dtc",
                                    "sys-kernel/dracut",
                                    "sys-apps/busybox",
                                    // Needed to assemble the whole image
                                    "sys-fs/genimage",
                                    "app-arch/xz-utils",
                                    // crossdev repository setup
                                    "app-eselect/eselect-repository",
                                ];

                                // Convert packages array to command arguments for single emerge call
                                let mut emerge_args = vec!["-v"];
                                for package in packages.iter() {
                                    emerge_args.push(package);
                                }

                                info!("Installing all cross-compilation prerequisites...");
                                let result = backend
                                    .run_command("default", "emerge", &emerge_args, None)
                                    .await;

                                match result {
                                    Ok(_) => info!("✓ All packages installed"),
                                    Err(e) => {
                                        eprintln!("Error: Failed to install packages: {}", e);
                                        std::process::exit(1);
                                    }
                                }

                                match result {
                                    Ok(_) => info!("✓ All packages installed"),
                                    Err(e) => {
                                        eprintln!("Warning: Failed to install packages: {}", e);
                                    }
                                }

                                // Set up crossdev repository
                                info!("Setting up crossdev repository...");
                                let repo_result = backend
                                    .run_command(
                                        "default",
                                        "eselect",
                                        &["repository", "create", "crossdev"],
                                        None,
                                    )
                                    .await;

                                match repo_result {
                                    Ok(_) => info!("✓ crossdev repository created"),
                                    Err(e) => {
                                        eprintln!(
                                            "Warning: Failed to create crossdev repository: {}",
                                            e
                                        );
                                    }
                                }

                                // Run emerge --sync to update package database
                                info!("Running emerge --sync to update package database...");
                                let sync_result = backend
                                    .run_command("default", "emerge", &["--sync"], None)
                                    .await;

                                match sync_result {
                                    Ok(_) => info!("✓ Package database synchronized"),
                                    Err(e) => {
                                        eprintln!("Error: Failed to sync package database: {}", e);
                                        eprintln!("  Package installation cannot proceed without current package information");
                                        std::process::exit(1);
                                    }
                                }

                                println!(
                                    "✓ Sandbox '{}' setup complete with backend: {}",
                                    name,
                                    backend.name()
                                );
                                println!("  Image: {}", image);
                                println!("  Status: Ready for cross-compilation preparation");
                                println!("\nNext steps:");
                                println!(
                                    "  1. Run 'sandbox prepare' to set up crossdev environment"
                                );
                                println!("   2. Or enter the sandbox with 'sandbox enter'");

                                // Track sandbox state
                                use stage::SandboxRegistry;
                                let registry_path = SandboxRegistry::get_default_registry_path();
                                let mut registry = SandboxRegistry::load_from_file(&registry_path)
                                    .unwrap_or_default();
                                let sandbox_state = SandboxRegistry::create_sandbox_state(
                                    &name,
                                    stage::SandboxStatus::New,
                                );
                                registry.upsert_sandbox(sandbox_state)?;
                                registry.save_to_file(&registry_path)?;
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

                SandboxCommands::Prepare(args) => {
                    let target = args.target;

                    info!(
                        "Preparing cross-compilation environment for target: {}",
                        target
                    );

                    match auto_detect_backend() {
                        Ok(backend) => {
                            if backend.name() == "docker" {
                                // Load platform configuration
                                let config_file = format!("config/platforms/{}.toml", target);
                                let config = match crossdev_config::PlatformConfig::load_from_file(
                                    &config_file,
                                ) {
                                    Ok(cfg) => cfg,
                                    Err(e) => {
                                        eprintln!("Failed to load platform config: {}", e);
                                        std::process::exit(1);
                                    }
                                };

                                let crossdev_root = format!("/usr/{}", config.compilation.chost);

                                info!(
                                    "Setting up crossdev environment for {}",
                                    config.compilation.chost
                                );

                                // Compute LLVM targets for both host and target
                                let host_llvm_target = arch_to_llvm_target(std::env::consts::ARCH);
                                let target_llvm_target =
                                    arch_to_llvm_target(&config.compilation.chost);

                                // Use our structured crossdev environment setup
                                let crossdev_env = CrossdevEnvironment::new(
                                    &config.compilation.chost,
                                    &crossdev_root,
                                    &config.compilation.profile,
                                    &config.compilation.cflags,
                                    &target_llvm_target,
                                );

                                match crossdev_env.initialize(&*backend).await {
                                    Ok(_) => {
                                        info!("✓ Crossdev environment setup complete");
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "Error: Failed to setup crossdev environment: {}",
                                            e
                                        );
                                        std::process::exit(1);
                                    }
                                }

                                // Workaround crossdev unmasking improperly
                                let unmask_result = backend.run_command(
                                    "default",
                                    "sh",
                                    &["-c", &format!(
                                        "mkdir -p /etc/portage/package.{{accept_keywords,mask}} && \
                                         echo \"cross-{}/rust-std **\" > /etc/portage/package.accept_keywords/rust-std && \
                                         echo \"=cross-{}/gcc-15*\" > /etc/portage/package.mask/cross-{}-fixup",
                                        config.compilation.chost, config.compilation.chost, config.compilation.chost
                                    )],
                                    None
                                ).await;

                                match unmask_result {
                                    Ok(_) => info!("✓ Crossdev unmasking workarounds applied"),
                                    Err(e) => {
                                        eprintln!(
                                            "Warning: Failed to apply unmasking workarounds: {}",
                                            e
                                        );
                                    }
                                }

                                // crossdev starts as split_usr layout - convert to merged usr
                                let merge_usr_result = backend
                                    .run_command(
                                        "default",
                                        "merge-usr",
                                        &["--root", &crossdev_root],
                                        None,
                                    )
                                    .await;

                                match merge_usr_result {
                                    Ok(_) => info!("✓ merge-usr completed"),
                                    Err(e) => {
                                        eprintln!("Warning: Failed to run merge-usr: {}", e);
                                        eprintln!("This is expected if merge-usr is not available in the container.");
                                    }
                                }

                                // Install crossdev packages
                                let install_result = backend
                                    .run_command(
                                        "default",
                                        "crossdev",
                                        &[
                                            config.compilation.chost.as_str(),
                                            "--g",
                                            &config.compilation.gcc_version,
                                            "--ex-pkg",
                                            "sys-devel/clang-crossdev-wrappers",
                                            "--ex-pkg",
                                            "sys-devel/rust-std",
                                        ],
                                        None,
                                    )
                                    .await;

                                match install_result {
                                    Ok(_) => info!("✓ Crossdev packages installed"),
                                    Err(e) => {
                                        eprintln!("Failed to install crossdev packages: {}", e);
                                        std::process::exit(1);
                                    }
                                }

                                // Set LLVM_TARGETS in host make.conf for native compilation
                                // Only include host and target architectures as directed

                                // Only use host and target LLVM targets
                                let mut unique_targets: Vec<&str> = vec![
                                    host_llvm_target.as_str(),   // Host architecture
                                    target_llvm_target.as_str(), // Target architecture
                                ];

                                // Deduplicate in case host and target are the same
                                unique_targets.sort();
                                unique_targets.dedup();

                                let host_llvm_targets = format!("\"{}\"", unique_targets.join(" "));

                                let host_llvm_result = backend
                                    .run_command(
                                        "default",
                                        "sh",
                                        &[
                                            "-c",
                                            &format!(
                                                "if [ -f /etc/portage/make.conf ]; then \
                                                   sed -i '/^LLVM_TARGETS=/d' /etc/portage/make.conf && \
                                                   echo 'LLVM_TARGETS={}' >> /etc/portage/make.conf; \
                                                 else \
                                                   echo 'LLVM_TARGETS={}' > /etc/portage/make.conf; \
                                                 fi",
                                                host_llvm_targets, host_llvm_targets
                                            ),
                                        ],
                                        None,
                                    )
                                    .await;

                                match host_llvm_result {
                                    Ok(_) => info!("✓ Host LLVM_TARGETS configured"),
                                    Err(e) => {
                                        eprintln!(
                                            "Warning: Failed to set host LLVM_TARGETS: {}",
                                            e
                                        );
                                        eprintln!("  Host LLVM compilation may be limited");
                                    }
                                }

                                println!("✓ Cross-compilation environment prepared successfully");
                                println!("  Target: {}", target);
                                println!("  CHOST: {}", config.compilation.chost);
                                println!("  Profile: {}", config.compilation.profile);
                                println!("  Status: Ready for cross-compilation");
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
                SandboxCommands::Enter(args) => {
                    let name = args.name;
                    let working_dir = args.working_dir;

                    info!("Entering sandbox '{}'", name);

                    match auto_detect_backend() {
                        Ok(backend) => {
                            if backend.name() == "docker" {
                                println!("✓ Entering Docker sandbox '{}'", name);

                                // Use bash -li for proper login interactive shell
                                let command = ["bash", "-li"];

                                let working_dir_path =
                                    working_dir.as_deref().map(std::path::Path::new);
                                match backend
                                    .exec_interactive(&name, &command, working_dir_path)
                                    .await
                                {
                                    Ok(_) => {
                                        println!("Interactive session completed");

                                        // Stop the container after use to free resources
                                        self::cleanup_container(&name).await;
                                    }
                                    Err(e) => {
                                        eprintln!("Failed to start interactive session: {}", e);
                                        self::cleanup_container(&name).await;
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
                                    working_dir.as_deref().unwrap_or("default")
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
                SandboxCommands::List => {
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
                SandboxCommands::Run(args) => {
                    let name = args.name;
                    let command = args.command;
                    let args_refs: Vec<&str> = args.args.iter().map(|s| s.as_str()).collect();
                    let working_dir = args.working_dir;

                    info!(
                        "Running command in sandbox '{}': {} {}",
                        name,
                        command,
                        args_refs.join(" ")
                    );

                    match auto_detect_backend() {
                        Ok(backend) => {
                            let working_dir_path = working_dir.as_deref().map(std::path::Path::new);
                            match backend
                                .run_command(&name, &command, &args_refs, working_dir_path)
                                .await
                            {
                                Ok(output) => {
                                    println!(
                                        "✓ Command executed successfully in sandbox '{}':",
                                        name
                                    );
                                    println!("{}", output);

                                    // Stop the container after use to free resources
                                    self::cleanup_container(&name).await;
                                }
                                Err(e) => {
                                    eprintln!(
                                        "Error executing command in sandbox '{}': {}",
                                        name, e
                                    );
                                    self::cleanup_container(&name).await;
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
                SandboxCommands::Delete(args) => {
                    let name = args.name;
                    let force = args.force;

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
                                let _ = docker.stop_container(&name, None).await;

                                match docker
                                    .remove_container(
                                        &name,
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

                                        // Remove from sandbox registry
                                        use stage::SandboxRegistry;
                                        let registry_path =
                                            SandboxRegistry::get_default_registry_path();
                                        let mut registry =
                                            SandboxRegistry::load_from_file(&registry_path)
                                                .unwrap_or_default();
                                        let _ = registry.remove_sandbox(&name);
                                        let _ = registry.save_to_file(&registry_path);

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
            }
        }
    }

    /// Handle stage list command with glob pattern support
    async fn handle_stage_list(args: StageListArgs) -> Result<(), Box<dyn std::error::Error>> {
        let cache_dir = args.cache;
        let arch_pattern = args.arch;
        let flavor_pattern = args.flavor;
        let detailed = args.detailed;
        let sandbox_filter = args.sandbox;

        // If sandbox filter is specified, show loaded stages for that sandbox
        if let Some(sandbox_name) = sandbox_filter {
            use stage::SandboxRegistry;

            info!("Listing stages loaded in sandbox: {}", sandbox_name);

            // Load sandbox registry
            let registry_path = SandboxRegistry::get_default_registry_path();
            let registry = SandboxRegistry::load_from_file(&registry_path).unwrap_or_default();

            // Find the sandbox
            if let Some(sandbox) = registry.get_sandbox(&sandbox_name) {
                println!("Sandbox: {}", sandbox_name);
                println!("  Status: {:?}", sandbox.state);
                println!("  Created: {}", sandbox.created_at);
                println!("  Updated: {}", sandbox.last_updated);

                if let Some(loaded_stage) = &sandbox.loaded_stage {
                    println!("  Loaded Stage: {}", loaded_stage);

                    // Try to show more info about the loaded stage
                    if detailed {
                        // Parse stage name to extract info
                        let parts: Vec<&str> = loaded_stage.split('-').collect();
                        if parts.len() >= 3 {
                            let arch = parts[1];
                            let flavor = parts[2];
                            println!("  Stage Architecture: {}", arch);
                            println!("  Stage Flavor: {}", flavor);
                        }
                    }
                } else {
                    println!("  Loaded Stage: None");
                }

                return Ok(());
            } else {
                println!("Sandbox '{}' not found in registry.", sandbox_name);
                println!(
                    "Note: Sandbox state tracking is automatic. If you just created this sandbox,"
                );
                println!("it will be tracked after the first update operation.");
                return Ok(());
            }
        }

        info!("Listing stage3 files in cache: {}", cache_dir);

        // Create cache directory if it doesn't exist
        fs::create_dir_all(&cache_dir)?;

        // Read all files in cache directory
        let mut entries = fs::read_dir(&cache_dir)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().map(|ft| ft.is_file()).unwrap_or(false))
            .collect::<Vec<_>>();

        // Sort entries by name
        entries.sort_by_key(|entry| entry.file_name());

        if entries.is_empty() {
            println!("No stage3 files found in cache: {}", cache_dir);
            return Ok(());
        }

        println!(
            "Stage3 files in cache: {} ({} file(s))",
            cache_dir,
            entries.len()
        );
        println!("==========================================");

        let mut matched_count = 0;

        for entry in entries {
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy();

            // Parse file name to extract arch and flavor
            // Expected format: stage3-{arch}-{flavor}-{date}.tar.xz
            let parts: Vec<&str> = file_name_str.split('-').collect();
            let arch = parts.get(1).copied().unwrap_or("unknown");
            let flavor = parts.get(2).copied().unwrap_or("unknown");

            // Apply glob pattern filtering
            let arch_match = arch_pattern
                .as_ref()
                .map(|pattern| {
                    Pattern::new(pattern)
                        .map(|p| p.matches(arch))
                        .unwrap_or(false)
                })
                .unwrap_or(true);

            let flavor_match = flavor_pattern
                .as_ref()
                .map(|pattern| {
                    Pattern::new(pattern)
                        .map(|p| p.matches(flavor))
                        .unwrap_or(false)
                })
                .unwrap_or(true);

            if arch_match && flavor_match {
                matched_count += 1;

                if detailed {
                    // Get file metadata for detailed view
                    let metadata = entry.metadata()?;
                    let size = metadata.len();

                    println!("File: {}", file_name_str);
                    println!("  Arch: {}", arch);
                    println!("  Flavor: {}", flavor);
                    println!("  Size: {} bytes", size);
                    println!();
                } else {
                    println!("- {} (arch: {}, flavor: {})", file_name_str, arch, flavor);
                }
            }
        }

        if matched_count == 0 {
            println!("No files matched the specified filters.");
        } else {
            println!("\nTotal: {} file(s) matched", matched_count);
        }
        Ok::<(), Box<dyn std::error::Error>>(())
    }

    /// Handle stage delete command with glob pattern support
    async fn handle_stage_delete(args: StageDeleteArgs) -> Result<(), Box<dyn std::error::Error>> {
        let cache_dir = args.cache;
        let patterns = args.patterns;
        let dry_run = args.dry_run;
        let force = args.force;

        info!("Searching for files to delete in: {}", cache_dir);

        // Create cache directory if it doesn't exist
        fs::create_dir_all(&cache_dir)?;

        // Find all files matching the patterns
        let mut files_to_delete = Vec::new();

        for pattern_str in &patterns {
            let pattern = Pattern::new(pattern_str);

            match pattern {
                Ok(pattern) => {
                    // Read all files in cache directory
                    let entries = fs::read_dir(&cache_dir)?
                        .filter_map(|entry| entry.ok())
                        .filter(|entry| entry.file_type().map(|ft| ft.is_file()).unwrap_or(false));

                    for entry in entries {
                        let file_name = entry.file_name();
                        let file_name_str = file_name.to_string_lossy();

                        if pattern.matches(&file_name_str) {
                            let file_path = format!("{}/{}", cache_dir, file_name_str);
                            files_to_delete.push(file_path);
                        }
                    }
                }
                Err(e) => {
                    warn!("Invalid glob pattern '{}': {}", pattern_str, e);
                }
            }
        }

        // Remove duplicates
        files_to_delete.sort();
        files_to_delete.dedup();

        if files_to_delete.is_empty() {
            println!("No files matched the specified patterns.");
            return Ok(());
        }

        println!(
            "Found {} file(s) matching deletion patterns:",
            files_to_delete.len()
        );
        for file in &files_to_delete {
            println!("  - {}", file);
        }

        // Check if we should proceed
        if dry_run {
            println!("\nDry run: No files were actually deleted.");
            return Ok(());
        }

        if !force && !confirm_deletion(&files_to_delete) {
            println!("Deletion cancelled by user.");
            return Ok(());
        }

        // Perform actual deletion
        let mut deleted_count = 0;
        let mut error_count = 0;

        for file in &files_to_delete {
            match fs::remove_file(file) {
                Ok(_) => {
                    println!("✓ Deleted: {}", file);
                    deleted_count += 1;
                }
                Err(e) => {
                    eprintln!("✗ Failed to delete {}: {}", file, e);
                    error_count += 1;
                }
            }
        }

        println!("\nDeletion complete:");
        println!("  Successfully deleted: {}", deleted_count);
        println!("  Failed to delete: {}", error_count);

        Ok(())
    }

    /// Handle stage load command using sandbox backend
    async fn handle_stage_load(args: StageLoadArgs) -> Result<(), Box<dyn std::error::Error>> {
        use jiff::Timestamp;

        let sandbox_name = args.sandbox;
        let stage_name = args.stage;
        let cache_dir = args.cache;
        let _force = args.force;

        info!(
            "Loading stage '{}' into sandbox '{}'",
            stage_name, sandbox_name
        );

        // Use sandbox backend instead of direct file operations
        let backend = auto_detect_backend()?;

        let stage_path = format!("{}/{}", cache_dir, stage_name);
        if !std::path::Path::new(&stage_path).exists() {
            return Err(format!("Stage file '{}' not found in cache", stage_name).into());
        }

        backend
            .load_stage3(&sandbox_name, std::path::Path::new(&stage_path))
            .await?;

        // Track sandbox state - update to StageLoaded
        let registry_path = stage::SandboxRegistry::get_default_registry_path();
        let mut registry = stage::SandboxRegistry::load_from_file(&registry_path)?;

        let sandbox_state = if let Some(existing) = registry.get_sandbox(&sandbox_name).cloned()
        {
            let mut state = existing;
            state.state = stage::SandboxStatus::StageLoaded;
            state.loaded_stage = Some(stage_name.clone());
            state.last_updated = Timestamp::now().strftime("%Y%m%dT%H").to_string();
            state
        } else {
            let mut state = stage::SandboxRegistry::create_sandbox_state(
                &sandbox_name,
                stage::SandboxStatus::StageLoaded,
            );
            state.loaded_stage = Some(stage_name.clone());
            state
        };

        registry.upsert_sandbox(sandbox_state)?;
        registry.save_to_file(&registry_path)?;

        println!("✓ Stage loaded: {} -> {}", stage_name, sandbox_name);
        Ok(())
    }

    /// Handle stage save command using sandbox backend
    async fn handle_stage_save(args: StageSaveArgs) -> Result<(), Box<dyn std::error::Error>> {
        let sandbox_name = args.sandbox;
        let stage_name = args.name;
        let cache_dir = args.cache;
        let _force = args.force;

        info!(
            "Saving sandbox '{}' as stage3 '{}'",
            sandbox_name, stage_name
        );

        // Use sandbox backend instead of direct file operations
        let backend = auto_detect_backend()?;

        let stage_path = format!("{}/{}", cache_dir, stage_name);

        // Create cache directory if it doesn't exist
        fs::create_dir_all(&cache_dir)?;

        backend
            .save_stage3(&sandbox_name, std::path::Path::new(&stage_path))
            .await?;

        info!(
            "Sandbox '{}' successfully saved as stage '{}'",
            sandbox_name, stage_name
        );
        println!("✓ Stage saved: {} -> {}", sandbox_name, stage_name);

        Ok(())
    }

    /// Handle stage wipe command using sandbox backend
    async fn handle_stage_wipe(args: StageWipeArgs) -> Result<(), Box<dyn std::error::Error>> {
        let sandbox_name = args.sandbox;
        let _force = args.force;

        info!("Wiping stage files from sandbox '{}'", sandbox_name);

        // Use sandbox backend instead of direct file operations
        let backend = auto_detect_backend()?;

        backend.wipe_stage3(&sandbox_name).await?;

        info!(
            "Stage files successfully wiped from sandbox '{}'",
            sandbox_name
        );
        println!("✓ Stage files wiped from: {}", sandbox_name);

        Ok(())
    }

    async fn handle_stage_update(args: StageUpdateArgs) -> Result<(), Box<dyn std::error::Error>> {
        use jiff::Timestamp;

        let sandbox_name = args.sandbox;
        let stage_dir = args.stage_dir;
        let update_ldconfig = args.ldconfig;
        let _force = args.force;

        info!("Updating stage in sandbox '{}'", sandbox_name);

        // Load platform configuration
        let platform_config = stage::get_default_platform_config()?;

        // Create stage manager
        let cache_dir = get_default_cache_dir();
        let mirror_url = "https://distfiles.gentoo.org";
        let stage_manager = StageManager::new(platform_config, cache_dir, mirror_url);

        // Stage directory is required for update operations
        // If it's a relative path, it's relative to the sandbox working directory
        // If it's an absolute path, use it as-is
        let stage_dir_path = if let Some(dir) = stage_dir {
            let path = PathBuf::from(dir);
            if path.is_absolute() {
                path
            } else {
                // For relative paths, we'll use them directly in the container
                // The backend will handle the container path resolution
                path
            }
        } else {
            return Err(
                "Stage directory must be specified for update operations. Use --stage-dir option."
                    .into(),
            );
        };

        info!("Updating stage at: {}", stage_dir_path.display());

        // Track sandbox state - set to Updating
        let registry_path = stage::SandboxRegistry::get_default_registry_path();
        let mut registry = stage::SandboxRegistry::load_from_file(&registry_path)?;

        let mut sandbox_state = if let Some(existing) = registry.get_sandbox(&sandbox_name).cloned()
        {
            let mut state = existing;
            state.state = stage::SandboxStatus::Updating;
            state.last_updated = Timestamp::now().strftime("%Y%m%dT%H").to_string();
            state
        } else {
            stage::SandboxRegistry::create_sandbox_state(
                &sandbox_name,
                stage::SandboxStatus::Updating,
            )
        };

        // Save the updating state
        registry.upsert_sandbox(sandbox_state.clone())?;
        registry.save_to_file(&registry_path)?;

        // Update the stage
        stage_manager.update_stage3(&stage_dir_path).await?;

        // Update ldconfig if requested
        if update_ldconfig {
            info!("Updating ldconfig cache...");
            stage_manager.update_ldconfig(&stage_dir_path).await?;
        }

        // Update sandbox state - set to StageLoaded
        sandbox_state.state = stage::SandboxStatus::StageLoaded;
        sandbox_state.loaded_stage = Some(stage_dir_path.to_string_lossy().into_owned());
        sandbox_state.last_updated = Timestamp::now().strftime("%Y%m%dT%H").to_string();

        registry.upsert_sandbox(sandbox_state)?;
        registry.save_to_file(&registry_path)?;

        info!("Stage update completed successfully");
        println!("✓ Stage updated in sandbox: {}", sandbox_name);

        Ok(())
    }

    async fn handle_list_sandboxes() -> Result<(), Box<dyn std::error::Error>> {
        use stage::SandboxRegistry;

        info!("Listing sandbox states...");

        // Load the sandbox registry
        let registry_path = SandboxRegistry::get_default_registry_path();
        let registry = SandboxRegistry::load_from_file(&registry_path)?;

        let sandboxes = registry.list_sandboxes();

        if sandboxes.is_empty() {
            println!("No sandboxes found.");
            return Ok(());
        }

        println!("Sandbox States:");
        println!("===============");

        // Try to get a sandbox backend for container inspection
        let backend_result = auto_detect_backend();
        
        for sandbox in sandboxes {
            println!("Name: {}", sandbox.name);
            println!("  Status: {:?}", sandbox.state);
            
            // Show loaded stage information
            if let Some(loaded_stage) = &sandbox.loaded_stage {
                println!("  Stage: {}", loaded_stage);
                
                // Try to inspect container to find stage directories with .origin info
                if let Ok(backend) = backend_result.as_ref() {
                    if backend.name() == "docker" {
                        match backend.inspect_stage_directories(&sandbox.name).await {
                            Ok(stage_info) => {
                                if !stage_info.is_empty() {
                                    println!("  Stage Directories:");
                                    for (stage_dir, origin_content) in stage_info {
                                        println!("    - /stages/{}", stage_dir);
                                        if let Some(origin) = origin_content {
                                            println!("      Origin: {}", origin);
                                        } else {
                                            println!("      Origin: (not found)");
                                        }
                                    }
                                } else {
                                    println!("  Stage Directories: (none found)");
                                }
                            }
                            Err(e) => {
                                println!("  Stage Inspection Error: {}", e);
                            }
                        }
                    }
                }
            } else {
                println!("  Stage: None");
            }
            
            println!("  Created: {}", sandbox.created_at);
            println!("  Updated: {}", sandbox.last_updated);
            println!();
        }

        println!("Total: {} sandbox(es)", sandboxes.len());

        Ok(())
    }

    /// Confirm deletion with user interaction
    fn confirm_deletion(files: &[String]) -> bool {
        if files.is_empty() {
            return false;
        }

        println!("\nThe following files will be deleted:");
        for (i, file) in files.iter().enumerate() {
            println!("  {}. {}", i + 1, file);
        }

        print!(
            "\nAre you sure you want to delete these {} file(s)? [y/N]: ",
            files.len()
        );
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        input.trim().eq_ignore_ascii_case("y")
    }

    Ok(())
}
