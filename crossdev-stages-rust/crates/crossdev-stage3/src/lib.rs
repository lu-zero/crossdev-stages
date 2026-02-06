//! Stage3 image fetching for crossdev-stages
//!
//! This crate handles fetching, caching, and extracting Gentoo stage3 images.

use crossdev_config::{ConfigError, PlatformConfig};
use log::info;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

/// Stage3 fetching and management errors
#[derive(Debug, Error)]
pub enum Stage3Error {
    #[error("Failed to fetch stage3 list: {0}")]
    FetchError(String),

    #[error("Failed to parse stage3 metadata: {0}")]
    ParseError(String),

    #[error("Failed to download stage3 image: {0}")]
    DownloadError(String),

    #[error("Failed to verify stage3 image: {0}")]
    VerifyError(String),

    #[error("Failed to extract stage3 image: {0}")]
    ExtractError(String),

    #[error("Configuration error: {0}")]
    ConfigError(#[from] ConfigError),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Information about a stage3 image
#[derive(Debug, Clone)]
pub struct Stage3Info {
    pub name: String,   // e.g., "stage3-riscv64-openrc-20231018T010001Z.tar.xz"
    pub url: String,    // Full download URL
    pub size: u64,      // Size in bytes
    pub date: String,   // Build date (extracted from filename)
    pub arch: String,   // Architecture (e.g., "riscv64")
    pub flavor: String, // Flavor (e.g., "rv64_lp64d-openrc")
}

/// Stage3 image fetcher and manager
pub struct Stage3Fetcher {
    config: PlatformConfig,
    cache_dir: PathBuf,
    mirror_url: String,
}

impl Stage3Fetcher {
    /// Create a new Stage3Fetcher
    ///
    /// # Arguments
    ///
    /// * `config` - Platform configuration
    /// * `cache_dir` - Directory to cache stage3 images
    /// * `mirror_url` - Gentoo mirror URL
    ///
    /// # Returns
    ///
    /// A new Stage3Fetcher instance
    pub fn new(config: PlatformConfig, cache_dir: impl AsRef<Path>, mirror_url: &str) -> Self {
        Self {
            config,
            cache_dir: cache_dir.as_ref().to_path_buf(),
            mirror_url: mirror_url.to_string(),
        }
    }

    /// List available stage3 flavors for the configured architecture
    ///
    /// This method fetches the general latest-stage3.txt file (not flavor-specific)
    /// to get all available flavors for the target architecture.
    ///
    /// # Returns
    ///
    /// A vector of available flavor strings
    pub fn list_available_flavors(&self) -> Result<Vec<String>, Stage3Error> {
        // Fetch all available stage3 images (not flavor-specific)
        let stage3_list = self.fetch_all_stage3_flavors()?;

        // Extract unique flavors from the list
        Ok(self.list_available_flavors_from_list(&stage3_list))
    }

    /// Helper method to extract unique flavors from a stage3 list
    ///
    /// This is used internally and for testing.
    ///
    /// # Arguments
    ///
    /// * `stage3_list` - List of stage3 images
    ///
    /// # Returns
    ///
    /// A vector of unique, sorted flavor strings
    fn list_available_flavors_from_list(&self, stage3_list: &[Stage3Info]) -> Vec<String> {
        let mut flavors = Vec::new();

        for stage3 in stage3_list {
            if !flavors.contains(&stage3.flavor) {
                flavors.push(stage3.flavor.clone());
            }
        }

        // Sort flavors alphabetically
        flavors.sort();

        flavors
    }

    /// Fetch the latest stage3 image for the configured architecture
    ///
    /// This method:
    /// 1. Fetches the list of available stage3 images
    /// 2. Finds the latest matching image
    /// 3. Downloads the image
    /// 4. Verifies the image
    ///
    /// # Returns
    ///
    /// Information about the fetched stage3 image
    pub fn fetch_latest(&self) -> Result<Stage3Info, Stage3Error> {
        // Fetch the list of available stage3 images
        let stage3_list = self.fetch_stage3_list()?;

        // Find the latest image for our target
        let latest = self.find_latest_stage3(&stage3_list)?;

        info!("Found latest stage3 image: {}", latest.name);

        // Check if we already have this image cached
        if self.is_cached(&latest) {
            info!("Stage3 image already cached: {}", latest.name);
            return Ok(latest);
        }

        // Download the image
        self.download_stage3(&latest)?;

        // Verify the image
        self.verify_stage3(&latest)?;

        Ok(latest)
    }

    /// Fetch the list of available stage3 images from Gentoo mirrors
    fn fetch_stage3_list(&self) -> Result<Vec<Stage3Info>, Stage3Error> {
        let latest_url = format!(
            "{}/releases/{}/autobuilds/latest-stage3-{}.txt",
            self.mirror_url.trim_end_matches('/'),
            self.config.target.arch,
            self.config.target.flavor
        );

        info!("Fetching stage3 list from: {}", latest_url);

        // Use curl to fetch the stage3 list
        let output = Command::new("curl")
            .arg("-s")
            .arg("-f")
            .arg(&latest_url)
            .output()
            .map_err(|e| Stage3Error::IoError(e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Stage3Error::FetchError(format!(
                "Failed to fetch stage3 list: {}",
                stderr
            )));
        }

        let content = String::from_utf8_lossy(&output.stdout);

        // Parse the stage3 list
        self.parse_stage3_list(&content)
    }

    /// Fetch all available stage3 images for the architecture (not flavor-specific)
    ///
    /// This method fetches the general latest-stage3.txt file that contains
    /// all available flavors for the target architecture.
    ///
    /// # Returns
    ///
    /// A vector of all available Stage3Info for all flavors
    fn fetch_all_stage3_flavors(&self) -> Result<Vec<Stage3Info>, Stage3Error> {
        // Fetch the general latest-stage3.txt file (not flavor-specific)
        let latest_url = format!(
            "{}/releases/{}/autobuilds/latest-stage3.txt",
            self.mirror_url.trim_end_matches('/'),
            self.config.target.arch
        );

        info!("Fetching all stage3 flavors from: {}", latest_url);

        // Use curl to fetch the general stage3 list
        let output = Command::new("curl")
            .arg("-s")
            .arg("-f")
            .arg(&latest_url)
            .output()
            .map_err(|e| Stage3Error::IoError(e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Stage3Error::FetchError(format!(
                "Failed to fetch all stage3 flavors: {}",
                stderr
            )));
        }

        let content = String::from_utf8_lossy(&output.stdout);

        // Parse the general stage3 list (contains all flavors)
        self.parse_all_flavors_list(&content)
    }

    /// Parse stage3 list content into Stage3Info structures (for all flavors)
    ///
    /// This method parses the general latest-stage3.txt file that contains
    /// all available flavors for the architecture.
    ///
    /// Example format:
    /// # Wed Oct 18 01:00:01 UTC 2023
    /// stage3-riscv64-openrc-20231018T010001Z.tar.xz 123456789 SHA256 abc123...
    fn parse_all_flavors_list(&self, content: &str) -> Result<Vec<Stage3Info>, Stage3Error> {
        let mut stage3_images = Vec::new();

        let mut in_pgp_section = false;

        for line in content.lines() {
            let line = line.trim();

            // Skip comments, empty lines, PGP headers, and PGP signature sections
            if line.is_empty() || line.starts_with('#') || line.starts_with("Hash:") {
                continue;
            }

            // Detect PGP sections
            if line == "-----BEGIN PGP SIGNED MESSAGE-----" {
                // This marks the start of signed content, but the content itself is valid
                continue;
            }

            if line == "-----BEGIN PGP SIGNATURE-----" {
                in_pgp_section = true;
                info!("PGP signature section: entered");
                continue;
            }

            if line == "-----END PGP SIGNATURE-----" {
                in_pgp_section = false;
                info!("PGP signature section: exited");
                continue;
            }

            // Skip lines in PGP signature sections (but not signed content)
            if in_pgp_section {
                continue;
            }

            info!("Processing line: {}", line);

            // Parse stage3 info
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let full_path = parts[0].to_string();

                // Parse size
                let size = parts[1].parse::<u64>().map_err(|e| {
                    Stage3Error::ParseError(format!(
                        "Failed to parse size for {}: {}",
                        full_path, e
                    ))
                })?;

                // Extract filename from path (format: timestamp/filename.tar.xz)
                let name = full_path
                    .split('/')
                    .last()
                    .unwrap_or(&full_path)
                    .to_string();

                // Extract arch and flavor from name
                if name.starts_with("stage3-") {
                    // Extract date from filename: stage3-arch-flavor-YYYYMMDDTHHMMSSZ.tar.xz
                    let date = extract_date_from_filename(&name);

                    // Extract actual flavor from filename
                    let actual_flavor = extract_flavor_from_filename(&name);

                    stage3_images.push(Stage3Info {
                        name: name.clone(),
                        url: format!(
                            "{}/releases/{}/autobuilds/{}",
                            self.mirror_url.trim_end_matches('/'),
                            self.config.target.arch,
                            full_path
                        ),
                        size,
                        date,
                        arch: self.config.target.arch.to_string(),
                        flavor: actual_flavor,
                    });
                }
            }
        }

        if stage3_images.is_empty() {
            return Err(Stage3Error::ParseError(format!(
                "No stage3 images found for arch={}",
                self.config.target.arch
            )));
        }

        Ok(stage3_images)
    }

    /// Parse stage3 list content into Stage3Info structures
    ///
    /// Example format:
    /// # Wed Oct 18 01:00:01 UTC 2023
    /// stage3-riscv64-openrc-20231018T010001Z.tar.xz 123456789 SHA256 abc123...
    fn parse_stage3_list(&self, content: &str) -> Result<Vec<Stage3Info>, Stage3Error> {
        let mut stage3_images = Vec::new();

        let mut in_pgp_section = false;

        for line in content.lines() {
            let line = line.trim();

            // Skip comments, empty lines, PGP headers, and PGP signature sections
            if line.is_empty() || line.starts_with('#') || line.starts_with("Hash:") {
                continue;
            }

            // Detect PGP sections
            if line == "-----BEGIN PGP SIGNED MESSAGE-----" {
                // This marks the start of signed content, but the content itself is valid
                continue;
            }

            if line == "-----BEGIN PGP SIGNATURE-----" {
                in_pgp_section = true;
                info!("PGP signature section: entered");
                continue;
            }

            if line == "-----END PGP SIGNATURE-----" {
                in_pgp_section = false;
                info!("PGP signature section: exited");
                continue;
            }

            // Skip lines in PGP signature sections (but not signed content)
            if in_pgp_section {
                continue;
            }

            info!("Processing line: {}", line);

            // Parse stage3 info
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let full_path = parts[0].to_string();

                // Parse size
                let size = parts[1].parse::<u64>().map_err(|e| {
                    Stage3Error::ParseError(format!(
                        "Failed to parse size for {}: {}",
                        full_path, e
                    ))
                })?;

                // Extract filename from path (format: timestamp/filename.tar.xz)
                let name = full_path
                    .split('/')
                    .last()
                    .unwrap_or(&full_path)
                    .to_string();

                // Extract arch and flavor from name
                if name.starts_with("stage3-") {
                    // Extract date from filename: stage3-arch-flavor-YYYYMMDDTHHMMSSZ.tar.xz
                    let date = extract_date_from_filename(&name);

                    // Extract actual flavor from filename
                    let actual_flavor = extract_flavor_from_filename(&name);

                    stage3_images.push(Stage3Info {
                        name: name.clone(),
                        url: format!(
                            "{}/releases/{}/autobuilds/{}",
                            self.mirror_url.trim_end_matches('/'),
                            self.config.target.arch,
                            full_path
                        ),
                        size,
                        date,
                        arch: self.config.target.arch.to_string(),
                        flavor: actual_flavor,
                    });
                }
            }
        }

        if stage3_images.is_empty() {
            return Err(Stage3Error::ParseError(format!(
                "No stage3 images found for arch={}, flavor={}",
                self.config.target.arch, self.config.target.flavor
            )));
        }

        Ok(stage3_images)
    }

    /// Find the latest stage3 image from a list
    ///
    /// The latest image is determined by the timestamp in the filename
    fn find_latest_stage3(&self, images: &[Stage3Info]) -> Result<Stage3Info, Stage3Error> {
        images
            .iter()
            .max_by(|a, b| {
                // Compare timestamps extracted from filenames
                let a_ts = extract_timestamp(&a.name);
                let b_ts = extract_timestamp(&b.name);
                a_ts.cmp(&b_ts)
            })
            .cloned()
            .ok_or_else(|| Stage3Error::ParseError("No stage3 images available".to_string()))
    }

    /// Check if a stage3 image is already cached
    fn is_cached(&self, stage3: &Stage3Info) -> bool {
        let cache_path = self.cache_dir.join(&stage3.name);
        cache_path.exists()
    }

    /// Download a stage3 image
    fn download_stage3(&self, stage3: &Stage3Info) -> Result<(), Stage3Error> {
        // Create cache directory if it doesn't exist
        std::fs::create_dir_all(&self.cache_dir).map_err(|e| Stage3Error::IoError(e))?;

        let cache_path = self.cache_dir.join(&stage3.name);

        info!("Downloading stage3 image: {}", stage3.name);
        info!("URL: {}", stage3.url);
        info!("Size: {} bytes", stage3.size);

        // Use curl to download the image
        let output = Command::new("curl")
            .arg("-L") // Follow redirects
            .arg("-o")
            .arg(&cache_path) // Output to cache file
            .arg(&stage3.url) // URL to download
            .output()
            .map_err(|e| Stage3Error::IoError(e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Stage3Error::DownloadError(format!(
                "Failed to download {}: {}",
                stage3.name, stderr
            )));
        }

        info!("Downloaded stage3 image to: {}", cache_path.display());

        Ok(())
    }

    /// Verify a downloaded stage3 image
    fn verify_stage3(&self, stage3: &Stage3Info) -> Result<(), Stage3Error> {
        let cache_path = self.cache_dir.join(&stage3.name);

        // Check if file exists
        if !cache_path.exists() {
            return Err(Stage3Error::VerifyError(format!(
                "Stage3 image not found: {}",
                cache_path.display()
            )));
        }

        // Check file size
        let metadata = std::fs::metadata(&cache_path).map_err(|e| Stage3Error::IoError(e))?;

        if metadata.len() != stage3.size {
            return Err(Stage3Error::VerifyError(format!(
                "Size mismatch for {}: expected {}, got {}",
                stage3.name,
                stage3.size,
                metadata.len()
            )));
        }

        // Additional verification could be added here:
        // - Checksum verification
        // - Signature verification
        // - File integrity checks

        info!("Stage3 image verified successfully: {}", stage3.name);

        Ok(())
    }

    /// Extract stage3 image to target directory
    ///
    /// # Arguments
    ///
    /// * `stage3` - Stage3 image information
    /// * `target_dir` - Directory to extract to
    ///
    /// # Returns
    ///
    /// Result indicating success or failure
    pub fn extract_stage3(
        &self,
        stage3: &Stage3Info,
        target_dir: impl AsRef<Path>,
    ) -> Result<(), Stage3Error> {
        let cache_path = self.cache_dir.join(&stage3.name);
        let target_dir = target_dir.as_ref();

        // Check if cache file exists
        if !cache_path.exists() {
            return Err(Stage3Error::ExtractError(format!(
                "Stage3 image not found in cache: {}",
                cache_path.display()
            )));
        }

        info!("Extracting stage3 image: {}", stage3.name);
        info!("Target directory: {}", target_dir.display());

        // Create target directory if it doesn't exist
        std::fs::create_dir_all(target_dir).map_err(|e| Stage3Error::IoError(e))?;

        // Use tar to extract the stage3 image
        // Exclude dev/ directory as in the original shell script
        let output = Command::new("tar")
            .arg("--exclude")
            .arg("dev/*")
            .arg("-xJpf")
            .arg(&cache_path) // Extract with xz decompression
            .arg("-C")
            .arg(target_dir) // Change to target directory
            .output()
            .map_err(|e| Stage3Error::IoError(e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Stage3Error::ExtractError(format!(
                "Failed to extract {}: {}",
                stage3.name, stderr
            )));
        }

        info!(
            "Stage3 image extracted successfully to: {}",
            target_dir.display()
        );

        Ok(())
    }

    /// Get cached stage3 images
    pub fn get_cached_images(&self) -> Result<Vec<Stage3Info>, Stage3Error> {
        let mut cached_images = Vec::new();

        // Read cache directory
        if !self.cache_dir.exists() {
            return Ok(cached_images);
        }

        for entry in std::fs::read_dir(&self.cache_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path.extension().map_or(false, |ext| ext == "xz") {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("stage3-") {
                        // This is a very basic reconstruction - in a real implementation,
                        // you'd want to parse the filename properly and potentially
                        // store metadata alongside cached files
                        cached_images.push(Stage3Info {
                            name: name.to_string(),
                            url: String::new(), // Would need to reconstruct or store
                            size: entry.metadata()?.len(),
                            date: "unknown".to_string(),
                            arch: self.config.target.arch.to_string(),
                            flavor: self.config.target.flavor.clone(),
                        });
                    }
                }
            }
        }

        Ok(cached_images)
    }

    /// Clear the stage3 cache
    pub fn clear_cache(&self) -> Result<(), Stage3Error> {
        if !self.cache_dir.exists() {
            return Ok(());
        }

        info!("Clearing stage3 cache: {}", self.cache_dir.display());

        for entry in std::fs::read_dir(&self.cache_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                std::fs::remove_file(&path).map_err(|e| Stage3Error::IoError(e))?;
            }
        }

        info!("Stage3 cache cleared successfully");

        Ok(())
    }
}

/// Extract timestamp from stage3 filename
///
/// Filename format: stage3-arch-flavor-YYYYMMDDTHHMMSSZ.tar.xz
/// Returns timestamp as u64 for comparison
fn extract_timestamp(filename: &str) -> u64 {
    // Split by '-' and get the timestamp part (second to last element)
    let parts: Vec<&str> = filename.split('-').collect();
    if parts.len() >= 4 {
        // Remove .tar.xz extension and T/Z characters
        let last_part = parts[parts.len() - 1];
        let timestamp_part = last_part
            .replace(".tar.xz", "")
            .replace("T", "")
            .replace("Z", "");

        if let Ok(ts) = timestamp_part.parse::<u64>() {
            return ts;
        }
    }
    0
}

/// Extract flavor from stage3 filename
///
/// Filename format: stage3-arch-flavor-YYYYMMDDTHHMMSSZ.tar.xz
/// Returns the flavor part (arch-flavor)
fn extract_flavor_from_filename(filename: &str) -> String {
    // Remove .tar.xz extension first
    let without_ext = filename.replace(".tar.xz", "");

    // Split by '-' and get the relevant parts
    let parts: Vec<&str> = without_ext.split('-').collect();

    if parts.len() >= 3 {
        // Format: stage3-arch-flavor-timestamp
        // We want parts[1] (arch) and parts[2] (flavor) combined
        return format!("{}-{}", parts[1], parts[2]);
    }

    // Fallback: return the full filename without extension and stage3 prefix
    without_ext.replace("stage3-", "")
}

/// Extract date from stage3 filename
///
/// Returns date as string in YYYYMMDD format
fn extract_date_from_filename(filename: &str) -> String {
    let parts: Vec<&str> = filename.split('-').collect();
    if parts.len() >= 4 {
        // Get the last part and remove .tar.xz extension
        let last_part = parts[parts.len() - 1];
        let timestamp_part = last_part.replace(".tar.xz", "");
        // Extract YYYYMMDD from YYYYMMDDTHHMMSSZ
        if timestamp_part.len() >= 8 {
            return timestamp_part[..8].to_string();
        }
    }
    "unknown".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossdev_config::PlatformConfig;
    use tempfile::tempdir;

    #[test]
    fn test_extract_timestamp() {
        let filename = "stage3-riscv64-openrc-20231018T010001Z.tar.xz";
        let timestamp = extract_timestamp(filename);
        assert_eq!(timestamp, 20231018010001);
    }

    #[test]
    fn test_extract_date_from_filename() {
        let filename = "stage3-riscv64-openrc-20231018T010001Z.tar.xz";
        let date = extract_date_from_filename(filename);
        assert_eq!(date, "20231018");
    }

    #[test]
    fn test_parse_stage3_list() {
        let config = PlatformConfig {
            target: crossdev_config::TargetConfig {
                arch: "riscv64".parse().unwrap(),
                flavor: "rv64_lp64d-openrc".to_string(),
            },
            compilation: crossdev_config::CompilationConfig {
                cflags: "test".to_string(),
                gcc_version: "test".to_string(),
                profile: "test".to_string(),
                chost: "riscv64-unknown-linux-gnu".to_string(),
                makeopts: "test".to_string(),
                emerge_default_opts: "test".to_string(),
            },
            repositories: crossdev_config::RepositoryConfig {
                opensbi_repo: "test".to_string(),
                opensbi_tag: "test".to_string(),
                u_boot_repo: "test".to_string(),
                u_boot_tag: "test".to_string(),
                firmware_repo: "test".to_string(),
                firmware_tag: "test".to_string(),
                kernel_repo: "test".to_string(),
                kernel_tag: "test".to_string(),
                bootloader_tag: "test".to_string(),
            },
            packages: crossdev_config::PackageConfig {
                stage1_file: "test".to_string(),
                additional_file: "test".to_string(),
            },
            image: crossdev_config::ImageConfig {
                root_size: "test".to_string(),
                boot_size: "test".to_string(),
                genimage_config: "test".to_string(),
            },
        };

        let fetcher = Stage3Fetcher::new(config, "/tmp/cache", "https://distfiles.gentoo.org");

        let test_data = r#"
# Wed Oct 18 01:00:01 UTC 2023
stage3-riscv64-openrc-20231018T010001Z.tar.xz 123456789 SHA256 abc123...
stage3-riscv64-openrc-20231017T010001Z.tar.xz 123456788 SHA256 def456...
"#;

        let result = fetcher.parse_stage3_list(test_data);
        assert!(result.is_ok());
        let images = result.unwrap();
        assert_eq!(images.len(), 2);
        assert!(images[0].name.contains("20231018"));
        assert!(images[1].name.contains("20231017"));
    }

    #[test]
    fn test_find_latest_stage3() {
        let config = PlatformConfig {
            target: crossdev_config::TargetConfig {
                arch: "riscv64".parse().unwrap(),
                flavor: "rv64_lp64d-openrc".to_string(),
            },
            compilation: crossdev_config::CompilationConfig {
                cflags: "test".to_string(),
                gcc_version: "test".to_string(),
                profile: "test".to_string(),
                chost: "riscv64-unknown-linux-gnu".to_string(),
                makeopts: "test".to_string(),
                emerge_default_opts: "test".to_string(),
            },
            repositories: crossdev_config::RepositoryConfig {
                opensbi_repo: "test".to_string(),
                opensbi_tag: "test".to_string(),
                u_boot_repo: "test".to_string(),
                u_boot_tag: "test".to_string(),
                firmware_repo: "test".to_string(),
                firmware_tag: "test".to_string(),
                kernel_repo: "test".to_string(),
                kernel_tag: "test".to_string(),
                bootloader_tag: "test".to_string(),
            },
            packages: crossdev_config::PackageConfig {
                stage1_file: "test".to_string(),
                additional_file: "test".to_string(),
            },
            image: crossdev_config::ImageConfig {
                root_size: "test".to_string(),
                boot_size: "test".to_string(),
                genimage_config: "test".to_string(),
            },
        };

        let fetcher = Stage3Fetcher::new(config, "/tmp/cache", "https://distfiles.gentoo.org");

        let images = vec![
            Stage3Info {
                name: "stage3-riscv64-openrc-20231017T010001Z.tar.xz".to_string(),
                url: "http://example.com/1.tar.xz".to_string(),
                size: 100,
                date: "20231017".to_string(),
                arch: "riscv64".to_string(),
                flavor: "rv64_lp64d-openrc".to_string(),
            },
            Stage3Info {
                name: "stage3-riscv64-openrc-20231018T010001Z.tar.xz".to_string(),
                url: "http://example.com/2.tar.xz".to_string(),
                size: 101,
                date: "20231018".to_string(),
                arch: "riscv64".to_string(),
                flavor: "rv64_lp64d-openrc".to_string(),
            },
        ];

        let latest = fetcher.find_latest_stage3(&images).unwrap();
        assert!(latest.name.contains("20231018"));
    }

    #[test]
    fn test_is_cached() {
        let dir = tempdir().unwrap();
        let cache_dir = dir.path().to_path_buf();

        // Create a fake cached file
        let cache_file = cache_dir.join("stage3-test.tar.xz");
        std::fs::write(&cache_file, "fake content").unwrap();

        let config = PlatformConfig {
            target: crossdev_config::TargetConfig {
                arch: "x86".parse().unwrap(),
                flavor: "test".to_string(),
            },
            compilation: crossdev_config::CompilationConfig {
                cflags: "test".to_string(),
                gcc_version: "test".to_string(),
                profile: "test".to_string(),
                chost: "test".to_string(),
                makeopts: "test".to_string(),
                emerge_default_opts: "test".to_string(),
            },
            repositories: crossdev_config::RepositoryConfig {
                opensbi_repo: "test".to_string(),
                opensbi_tag: "test".to_string(),
                u_boot_repo: "test".to_string(),
                u_boot_tag: "test".to_string(),
                firmware_repo: "test".to_string(),
                firmware_tag: "test".to_string(),
                kernel_repo: "test".to_string(),
                kernel_tag: "test".to_string(),
                bootloader_tag: "test".to_string(),
            },
            packages: crossdev_config::PackageConfig {
                stage1_file: "test".to_string(),
                additional_file: "test".to_string(),
            },
            image: crossdev_config::ImageConfig {
                root_size: "test".to_string(),
                boot_size: "test".to_string(),
                genimage_config: "test".to_string(),
            },
        };

        let fetcher = Stage3Fetcher::new(config, cache_dir, "https://distfiles.gentoo.org");

        let stage3 = Stage3Info {
            name: "stage3-test.tar.xz".to_string(),
            url: "test".to_string(),
            size: 100,
            date: "test".to_string(),
            arch: "test".to_string(),
            flavor: "test".to_string(),
        };

        assert!(fetcher.is_cached(&stage3));
    }

    #[test]
    fn test_list_available_flavors() {
        let config = PlatformConfig {
            target: crossdev_config::TargetConfig {
                arch: "riscv64".parse().unwrap(),
                flavor: "rv64_lp64d-openrc".to_string(),
            },
            compilation: crossdev_config::CompilationConfig {
                cflags: "test".to_string(),
                gcc_version: "test".to_string(),
                profile: "test".to_string(),
                chost: "riscv64-unknown-linux-gnu".to_string(),
                makeopts: "test".to_string(),
                emerge_default_opts: "test".to_string(),
            },
            repositories: crossdev_config::RepositoryConfig {
                opensbi_repo: "test".to_string(),
                opensbi_tag: "test".to_string(),
                u_boot_repo: "test".to_string(),
                u_boot_tag: "test".to_string(),
                firmware_repo: "test".to_string(),
                firmware_tag: "test".to_string(),
                kernel_repo: "test".to_string(),
                kernel_tag: "test".to_string(),
                bootloader_tag: "test".to_string(),
            },
            packages: crossdev_config::PackageConfig {
                stage1_file: "test".to_string(),
                additional_file: "test".to_string(),
            },
            image: crossdev_config::ImageConfig {
                root_size: "test".to_string(),
                boot_size: "test".to_string(),
                genimage_config: "test".to_string(),
            },
        };

        let fetcher = Stage3Fetcher::new(config, "/tmp/cache", "https://distfiles.gentoo.org");

        // Create mock stage3 list with multiple flavors
        let test_data = r#"
# Wed Oct 18 01:00:01 UTC 2023
stage3-riscv64-openrc-20231018T010001Z.tar.xz 123456789 SHA256 abc123...
stage3-riscv64-hardened-20231017T010001Z.tar.xz 123456788 SHA256 def456...
stage3-riscv64-openrc-20231016T010001Z.tar.xz 123456787 SHA256 ghi789...
"#;

        // Mock the fetch_stage3_list method to return our test data
        // For this test, we'll directly test the flavor extraction logic
        let stage3_list = fetcher.parse_stage3_list(test_data).unwrap();

        // Test the list_available_flavors method
        let flavors = fetcher.list_available_flavors_from_list(&stage3_list);

        assert_eq!(flavors.len(), 2);
        assert!(flavors.contains(&"riscv64-openrc".to_string()));
        assert!(flavors.contains(&"riscv64-hardened".to_string()));
        assert_eq!(flavors[0], "riscv64-hardened".to_string());
        assert_eq!(flavors[1], "riscv64-openrc".to_string());
    }
}
