use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Board configuration loaded from `boards/<name>/board.conf`.
#[derive(Debug, Clone)]
pub struct BoardConfig {
    pub name: String,
    pub arch: String,           // e.g. "riscv64"
    pub cflags: Option<String>, // BOARD_CFLAGS; None → use default_cflags(arch)
    pub cross_compile: String,  // e.g. "riscv64-unknown-linux-gnu-"
    pub kernel_arch: String,    // e.g. "riscv", "arm64", "x86"

    // OpenSBI
    pub opensbi_repo: Option<String>,
    pub opensbi_tag: Option<String>,
    pub opensbi_platform: Option<String>,

    // U-Boot
    pub u_boot_repo: Option<String>,
    pub u_boot_tag: Option<String>,
    pub u_boot_defconfig: Option<String>,

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
}

impl BoardConfig {
    /// Derive the CHOST triple from the arch (e.g. "riscv64-unknown-linux-gnu").
    pub fn chost(&self) -> String {
        format!("{}-unknown-linux-gnu", self.arch)
    }

    /// Effective CFLAGS (board-specific or arch default).
    pub fn effective_cflags(&self) -> String {
        self.cflags
            .clone()
            .unwrap_or_else(|| crate::stage::default_cflags(&self.arch).to_string())
    }

    /// Path to the board directory relative to the project root.
    pub fn board_dir<'a>(&self, boards_root: &'a Path) -> PathBuf {
        boards_root.join(&self.name)
    }

    /// Does this board have a `board.sh` with override functions?
    pub fn has_board_sh(&self, boards_root: &Path) -> bool {
        boards_root.join(&self.name).join("board.sh").exists()
    }
}

/// Load a board configuration from `<boards_root>/<name>/board.conf`.
pub fn load(boards_root: &Path, name: &str) -> Result<BoardConfig> {
    let path = boards_root.join(name).join("board.conf");
    let content = std::fs::read_to_string(&path).map_err(|e| Error::BoardNotFound(format!(
        "{}: {e}",
        path.display()
    )))?;
    parse(name, &path, &content)
}

/// List all board names found under `<boards_root>/*/board.conf`.
pub fn list(boards_root: &Path) -> Result<Vec<String>> {
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

fn parse(name: &str, path: &Path, content: &str) -> Result<BoardConfig> {
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
                file: path.display().to_string(),
                msg: format!("missing required field '{}'", $k),
            })?
        };
    }

    Ok(BoardConfig {
        name: name.to_string(),
        arch: req!("BOARD_ARCH"),
        cflags: kv.get("BOARD_CFLAGS").cloned(),
        cross_compile: req!("CROSS_COMPILE"),
        kernel_arch: req!("KERNEL_ARCH"),

        opensbi_repo: kv.get("OPENSBI_REPO").cloned(),
        opensbi_tag: kv.get("OPENSBI_TAG").cloned(),
        opensbi_platform: kv.get("OPENSBI_PLATFORM").cloned(),

        u_boot_repo: kv.get("U_BOOT_REPO").cloned(),
        u_boot_tag: kv.get("U_BOOT_TAG").or_else(|| kv.get("TAG")).cloned(),
        u_boot_defconfig: kv.get("U_BOOT_DEFCONFIG").cloned(),

        firmware_repo: kv.get("FIRMWARE_REPO").cloned(),
        firmware_overlay: kv.get("BOARD_FIRMWARE_OVERLAY").cloned(),
        host_firmware_paths: arrays.get("HOST_FIRMWARE_PATHS").cloned().unwrap_or_default(),

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
        hostname: kv.get("BOOT_HOSTNAME").cloned().unwrap_or_else(|| "gentoo".into()),
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
    })
}

/// Strip surrounding `"…"` or `'…'` quotes.
fn unquote(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"'))
        || (s.starts_with('\'') && s.ends_with('\''))
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
            let end = remaining.find(char::is_whitespace).unwrap_or(remaining.len());
            result.push(remaining[..end].to_string());
            remaining = remaining[end..].trim_start();
        }
    }
    result
}
