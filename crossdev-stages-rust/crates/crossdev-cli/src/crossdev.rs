//! Crossdev environment setup

use log::info;
use thiserror::Error;

/// Crossdev setup errors
#[derive(Debug, Error)]
pub enum CrossdevError {
    #[error("Crossdev initialization failed: {0}")]
    InitializationFailed(String),
    #[error("Profile configuration failed: {0}")]
    ProfileConfigurationFailed(String),
    #[error("Configuration file error: {0}")]
    ConfigFileError(String),
    #[error("Directory creation failed: {0}")]
    DirectoryCreationFailed(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Crossdev environment manager
pub struct CrossdevEnvironment {
    target: String,
    root: String,
    profile: String,
}

impl CrossdevEnvironment {
    /// Create a new CrossdevEnvironment instance
    pub fn new(target: &str, root: &str, profile: &str) -> Self {
        Self {
            target: target.to_string(),
            root: root.to_string(),
            profile: profile.to_string(),
        }
    }

    /// Initialize crossdev environment
    pub async fn initialize(
        &self,
        backend: &dyn crossdev_sandbox::SandboxBackend,
    ) -> Result<(), CrossdevError> {
        info!("Setting up crossdev environment for {}", self.target);

        // Step 1: Initialize crossdev
        self.init_crossdev(backend).await?;

        // Step 2: Set profile
        self.set_profile(backend).await?;

        // Step 3: Configure make.conf
        self.configure_make_conf(backend).await?;

        // Step 4: Setup directory structure and configuration files
        self.setup_configuration(backend).await?;

        info!("✓ Crossdev environment setup complete");
        Ok(())
    }

    /// Initialize crossdev
    async fn init_crossdev(
        &self,
        backend: &dyn crossdev_sandbox::SandboxBackend,
    ) -> Result<(), CrossdevError> {
        info!("Initializing crossdev for target {}", self.target);

        let result = backend
            .run_command(
                "default",
                "crossdev",
                &[&self.target, "--init-target"],
                None,
            )
            .await;

        match result {
            Ok(_) => {
                info!("✓ Crossdev initialized");
                Ok(())
            }
            Err(e) => Err(CrossdevError::InitializationFailed(e.to_string())),
        }
    }

    /// Set Gentoo profile
    async fn set_profile(
        &self,
        backend: &dyn crossdev_sandbox::SandboxBackend,
    ) -> Result<(), CrossdevError> {
        info!("Setting Gentoo profile to {}", self.profile);

        let result = backend
            .run_command(
                "default",
                "sh",
                &[
                    "-c",
                    &format!(
                        "PORTAGE_CONFIGROOT={} eselect profile set {}",
                        self.root, self.profile
                    ),
                ],
                None,
            )
            .await;

        match result {
            Ok(_) => {
                info!("✓ Profile configured");
                Ok(())
            }
            Err(e) => Err(CrossdevError::ProfileConfigurationFailed(e.to_string())),
        }
    }

    /// Configure make.conf
    async fn configure_make_conf(
        &self,
        backend: &dyn crossdev_sandbox::SandboxBackend,
    ) -> Result<(), CrossdevError> {
        info!("Configuring make.conf");

        // Set CFLAGS (using default optimization for now)
        let cflags = "-O3 -pipe";

        let result = backend
            .run_command(
                "default",
                "sh",
                &[
                    "-c",
                    &format!(
                        "echo 'CFLAGS=\"{}\"' > {}/etc/portage/make.conf",
                        cflags, self.root
                    ),
                ],
                None,
            )
            .await;

        if result.is_err() {
            return Err(CrossdevError::ConfigFileError(
                "Failed to set CFLAGS".to_string(),
            ));
        }

        // Add LLVM_TARGETS
        let result = backend
            .run_command(
                "default",
                "sh",
                &[
                    "-c",
                    &format!(
                        "echo 'LLVM_TARGETS=\"AArch64 RISCV\"' >> {}/etc/portage/make.conf",
                        self.root
                    ),
                ],
                None,
            )
            .await;

        match result {
            Ok(_) => {
                info!("✓ make.conf configured");
                Ok(())
            }
            Err(e) => Err(CrossdevError::ConfigFileError(e.to_string())),
        }
    }

    /// Setup directory structure and configuration files
    async fn setup_configuration(
        &self,
        backend: &dyn crossdev_sandbox::SandboxBackend,
    ) -> Result<(), CrossdevError> {
        info!("Setting up configuration files and directories");

        // Create directories
        self.create_directory(backend, "env").await?;
        self.create_directory(backend, "package.env").await?;
        self.create_directory(backend, "package.use").await?;
        self.create_directory(backend, "package.accept_keywords")
            .await?;

        // Create plain.conf
        self.create_plain_conf(backend).await?;

        // Create package.env configurations
        self.create_package_env(backend).await?;

        // Create package.use configurations
        self.create_package_use(backend).await?;

        info!("✓ Configuration files created");
        Ok(())
    }

    /// Create directory structure
    async fn create_directory(
        &self,
        backend: &dyn crossdev_sandbox::SandboxBackend,
        subdir: &str,
    ) -> Result<(), CrossdevError> {
        let path = format!("{}/etc/portage/{}", self.root, subdir);

        let result = backend
            .run_command("default", "mkdir", &["-p", &path], None)
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(CrossdevError::DirectoryCreationFailed(format!(
                "Failed to create {}: {}",
                path, e
            ))),
        }
    }

    /// Create plain.conf with optimization flags
    async fn create_plain_conf(
        &self,
        backend: &dyn crossdev_sandbox::SandboxBackend,
    ) -> Result<(), CrossdevError> {
        let plain_conf_content = "CFLAGS=\"-O3 -pipe\"\nCXXFLAGS=\"-O3 -pipe\"";
        let path = format!("{}/etc/portage/env/plain.conf", self.root);

        let result = backend
            .run_command(
                "default",
                "sh",
                &["-c", &format!("echo '{}' > {}", plain_conf_content, path)],
                None,
            )
            .await;

        match result {
            Ok(_) => {
                info!("✓ plain.conf created");
                Ok(())
            }
            Err(e) => Err(CrossdevError::ConfigFileError(format!(
                "Failed to create plain.conf: {}",
                e
            ))),
        }
    }

    /// Create package.env configurations
    async fn create_package_env(
        &self,
        backend: &dyn crossdev_sandbox::SandboxBackend,
    ) -> Result<(), CrossdevError> {
        // Configure rust to use plain.conf
        let rust_env_content = "dev-lang/rust plain.conf";
        let path = format!("{}/etc/portage/package.env/rust", self.root);

        let result = backend
            .run_command(
                "default",
                "sh",
                &["-c", &format!("echo '{}' > {}", rust_env_content, path)],
                None,
            )
            .await;

        match result {
            Ok(_) => {
                info!("✓ package.env configurations created");
                Ok(())
            }
            Err(e) => Err(CrossdevError::ConfigFileError(format!(
                "Failed to create package.env: {}",
                e
            ))),
        }
    }

    /// Create package.use configurations
    async fn create_package_use(
        &self,
        backend: &dyn crossdev_sandbox::SandboxBackend,
    ) -> Result<(), CrossdevError> {
        // Busybox configuration
        let busybox_content = ">=virtual/libcrypt-2-r1 static-libs\n>=sys-libs/libxcrypt-4.4.36-r3 static-libs\n>=sys-apps/busybox-1.36.1-r3 -pam static";
        let busybox_path = format!("{}/etc/portage/package.use/busybox", self.root);

        let result = backend
            .run_command(
                "default",
                "sh",
                &[
                    "-c",
                    &format!("echo -e '{}' > {}", busybox_content, busybox_path),
                ],
                None,
            )
            .await;

        if result.is_err() {
            return Err(CrossdevError::ConfigFileError(
                "Failed to create busybox package.use".to_string(),
            ));
        }

        // Clang configuration
        let clang_content = "llvm-core/clang -extra";
        let clang_path = format!("{}/etc/portage/package.use/clang", self.root);

        let result = backend
            .run_command(
                "default",
                "sh",
                &["-c", &format!("echo '{}' > {}", clang_content, clang_path)],
                None,
            )
            .await;

        if result.is_err() {
            return Err(CrossdevError::ConfigFileError(
                "Failed to create clang package.use".to_string(),
            ));
        }

        // Rust configuration
        let rust_content = "dev-lang/rust rustfmt -system-llvm";
        let rust_path = format!("{}/etc/portage/package.use/rust", self.root);

        let result = backend
            .run_command(
                "default",
                "sh",
                &["-c", &format!("echo '{}' > {}", rust_content, rust_path)],
                None,
            )
            .await;

        match result {
            Ok(_) => {
                info!("✓ package.use configurations created");
                Ok(())
            }
            Err(e) => Err(CrossdevError::ConfigFileError(format!(
                "Failed to create rust package.use: {}",
                e
            ))),
        }
    }
}
