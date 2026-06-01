use camino::Utf8Path;

use crate::error::{Error, Result};

/// Board configuration loaded from `boards/<name>/board.conf`.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BoardConfig {
    pub name: String,
    pub arch: String,                // e.g. "riscv64"
    pub chost_override: Option<String>, // CHOST; overrides derived chost_for_arch()
    pub cflags: Option<String>,      // BOARD_CFLAGS; None → use default_cflags(arch)
    pub ldflags: Option<String>, // BOARD_LDFLAGS; probably never needed (profile default is fine)
    pub rustflags: Option<String>, // BOARD_RUSTFLAGS; cross-compile target-cpu is handled by rust-std
    pub gcc_version: Option<String>, // BOARD_GCC_VERSION; None → highest installed slot
    pub cross_compile: String,     // e.g. "riscv64-unknown-linux-gnu-"
    pub kernel_arch: Option<String>, // e.g. "riscv", "arm64", "x86" — required for image builds

    // OpenSBI
    pub opensbi_repo: Option<String>,
    pub opensbi_tag: Option<String>,
    pub opensbi_platform: Option<String>,
    pub opensbi_fw_type: Option<String>,     // dynamic (default) | jump | payload
    pub opensbi_make_flags: Option<String>,  // extra make args

    // U-Boot
    pub u_boot_repo: Option<String>,
    pub u_boot_tag: Option<String>,
    pub u_boot_defconfig: Option<String>,
    pub u_boot_make_flags: Option<String>,   // extra make args

    // Firmware overlay
    pub firmware_repo: Option<String>,
    pub firmware_overlay: Option<String>, // path inside firmware repo
    pub host_firmware_paths: Vec<String>, // host paths to copy into image

    // Kernel
    pub kernel_repo: String,
    pub kernel_tag: String,
    pub kernel_defconfig: String,
    pub kernel_dtb_glob: Option<String>,

    pub dracut_modules: Option<String>,

    // Boot configuration
    pub root_dev: Option<String>,
    pub console: Option<String>,
    pub hostname: String,
    pub serial_tty: Option<String>,
    pub serial_baud: Option<String>,
    pub kernel_name: Option<String>,
    pub ramdisk_name: Option<String>,
    pub loglevel: Option<String>,

    pub services: Vec<String>, // e.g. ["sshd:default", "metalog:default"]
    pub build_steps: Vec<String>,

    // Per-package CFLAGS workarounds
    pub workaround_pkgs: Vec<String>,
    pub workaround_cflags: Vec<String>,

    pub image_name: Option<String>,
    pub compression: Option<String>,  // xz (default) | gz | none
    pub testing: bool,
}

impl BoardConfig {
    /// Derive the CHOST triple from the arch (e.g. "i586-pc-linux-gnu", "riscv64-unknown-linux-gnu").
    /// Uses explicit CHOST from board.conf if set, otherwise derives from arch.
    pub fn chost(&self) -> String {
        if let Some(ref chost) = self.chost_override {
            return chost.clone();
        }
        crate::stage::chost_for_arch(&self.arch).unwrap_or_else(|_| {
            format!("{}-unknown-linux-gnu", self.arch)
        })
    }

    /// Effective CFLAGS (board-specific or arch default).
    pub fn effective_cflags(&self) -> String {
        self.cflags
            .clone()
            .unwrap_or_else(|| crate::stage::default_cflags(&self.arch).to_string())
    }
}

/// Load a board configuration from `<boards_root>/<name>/board.conf`.
pub fn load(boards_root: &Utf8Path, name: &str) -> Result<BoardConfig> {
    let path = boards_root.join(name).join("board.conf");
    let content = std::fs::read_to_string(&path)
        .map_err(|e| Error::BoardNotFound(format!("{path}: {e}")))?;
    parse(name, &path, &content)
}

/// List all board names found under `<boards_root>/*/board.conf`.
pub fn list(boards_root: &Utf8Path) -> Result<Vec<String>> {
    if !boards_root.is_dir() {
        return Ok(vec![]);
    }
    let mut names: Vec<String> = std::fs::read_dir(boards_root)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("board.conf").exists())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    Ok(names)
}

// ── Parser ──────────────────────────────────────────────────────────────────

fn parse(name: &str, path: &Utf8Path, content: &str) -> Result<BoardConfig> {
    let mut kv = std::collections::HashMap::<String, String>::new();
    let mut arrays = std::collections::HashMap::<String, Vec<String>>::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, rest)) = line.split_once('=') {
            let key = key.trim().to_string();
            let rest = rest.trim();
            if rest.starts_with('(') {
                // Bash array
                let inner = rest.trim_start_matches('(').trim_end_matches(')');
                arrays.insert(key, parse_array(inner));
            } else {
                kv.insert(key, unquote(rest));
            }
        }
    }

    let tag = kv.get("TAG").cloned().unwrap_or_default();

    macro_rules! req {
        ($k:expr) => {
            kv.get($k).cloned().ok_or_else(|| Error::BoardConfigParse {
                file: path.to_string(),
                msg: format!("missing required field '{}'", $k),
            })?
        };
    }

    Ok(BoardConfig {
        name: name.to_string(),
        arch: req!("BOARD_ARCH"),
        chost_override: kv.get("CHOST").cloned(),
        cflags: kv.get("BOARD_CFLAGS").cloned(),
        ldflags: kv.get("BOARD_LDFLAGS").cloned(),
        rustflags: kv.get("BOARD_RUSTFLAGS").cloned(),
        gcc_version: kv.get("BOARD_GCC_VERSION").cloned(),
        cross_compile: req!("CROSS_COMPILE"),
        kernel_arch: kv.get("KERNEL_ARCH").cloned(),

        opensbi_repo: kv.get("OPENSBI_REPO").cloned(),
        opensbi_tag: kv.get("OPENSBI_TAG").cloned(),
        opensbi_platform: kv.get("OPENSBI_PLATFORM").cloned(),
        opensbi_fw_type: kv.get("OPENSBI_FW_TYPE").cloned(),
        opensbi_make_flags: kv.get("OPENSBI_MAKE_FLAGS").cloned(),

        u_boot_repo: kv.get("U_BOOT_REPO").cloned(),
        u_boot_tag: kv.get("U_BOOT_TAG").or_else(|| kv.get("TAG")).cloned(),
        u_boot_defconfig: kv.get("U_BOOT_DEFCONFIG").cloned(),
        u_boot_make_flags: kv.get("U_BOOT_MAKE_FLAGS").cloned(),

        firmware_repo: kv.get("FIRMWARE_REPO").cloned(),
        firmware_overlay: kv.get("BOARD_FIRMWARE_OVERLAY").cloned(),
        host_firmware_paths: arrays
            .get("HOST_FIRMWARE_PATHS")
            .cloned()
            .unwrap_or_default(),

        kernel_repo: req!("KERNEL_REPO"),
        kernel_tag: kv
            .get("KERNEL_TAG")
            .or_else(|| kv.get("TAG"))
            .cloned()
            .unwrap_or(tag.clone()),
        kernel_defconfig: req!("KERNEL_DEFCONFIG"),
        kernel_dtb_glob: kv.get("BOARD_DTB_GLOB").cloned(),

        dracut_modules: kv.get("DRACUT_MODULES").cloned(),

        root_dev: kv.get("BOOT_ROOT_DEV").cloned(),
        console: kv.get("BOOT_CONSOLE").cloned(),
        hostname: kv
            .get("BOOT_HOSTNAME")
            .cloned()
            .unwrap_or_else(|| "gentoo".into()),
        serial_tty: kv.get("BOOT_SERIAL_TTY").cloned(),
        serial_baud: kv.get("BOOT_SERIAL_BAUD").cloned(),
        kernel_name: kv.get("BOOT_KERNEL_NAME").cloned(),
        ramdisk_name: kv.get("BOOT_RAMDISK_NAME").cloned(),
        loglevel: kv.get("BOOT_LOGLEVEL").cloned(),

        services: arrays.get("BOOT_SERVICES").cloned().unwrap_or_default(),
        build_steps: arrays.get("BUILD_STEPS").cloned().unwrap_or_default(),

        workaround_pkgs: arrays.get("WORKAROUND_PKGS").cloned().unwrap_or_default(),
        workaround_cflags: arrays.get("WORKAROUND_CFLAGS").cloned().unwrap_or_default(),

        image_name: kv.get("IMAGE_NAME").cloned(),
        compression: kv.get("COMPRESSION").cloned(),
        testing: kv
            .get("TESTING")
            .map(|v| v == "true" || v == "yes" || v == "1")
            .unwrap_or(false),
    })
}

/// Strip surrounding `"…"` or `'…'` quotes.
fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Parse a bash array literal's inner content, e.g.:
///   `"sshd:default" "metalog:default"`  → vec!["sshd:default", "metalog:default"]
///   `deps checkout bootloader`           → vec!["deps", "checkout", "bootloader"]
fn parse_array(inner: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut remaining = inner.trim();
    while !remaining.is_empty() {
        if remaining.starts_with('"') {
            // Quoted element
            remaining = &remaining[1..];
            if let Some(end) = remaining.find('"') {
                result.push(remaining[..end].to_string());
                remaining = remaining[end + 1..].trim_start();
            } else {
                break;
            }
        } else if remaining.starts_with('\'') {
            remaining = &remaining[1..];
            if let Some(end) = remaining.find('\'') {
                result.push(remaining[..end].to_string());
                remaining = remaining[end + 1..].trim_start();
            } else {
                break;
            }
        } else {
            // Unquoted element (space-delimited)
            let end = remaining
                .find(char::is_whitespace)
                .unwrap_or(remaining.len());
            result.push(remaining[..end].to_string());
            remaining = remaining[end..].trim_start();
        }
    }
    result
}
