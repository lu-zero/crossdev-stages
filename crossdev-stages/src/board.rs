use camino::Utf8Path;

use crate::error::{Error, Result};
use crate::provider::{RootfsProvider, SecondStage};

/// Board configuration loaded from `boards/<name>/board.conf`.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BoardConfig {
    pub name: String,
    pub arch: String,                   // e.g. "riscv64"
    pub chost_override: Option<String>, // CHOST; overrides derived chost_for_arch()
    pub cflags: Option<String>,         // BOARD_CFLAGS; None → use default_cflags(arch)
    /// INCLUDE, kept so hooks can source the same files the loader merged.
    pub includes: Vec<String>,
    pub ldflags: Option<String>, // BOARD_LDFLAGS; probably never needed (profile default is fine)
    pub rustflags: Option<String>, // BOARD_RUSTFLAGS; cross-compile target-cpu is handled by rust-std
    pub gcc_version: Option<String>, // BOARD_GCC_VERSION; None → highest installed slot
    pub cross_compile: String,     // e.g. "riscv64-unknown-linux-gnu-"
    pub kernel_arch: Option<String>, // e.g. "riscv", "arm64", "x86" — required for image builds
    pub rootfs_provider: RootfsProvider, // ROOTFS_PROVIDER; absent → Gentoo
    /// Suite the debootstrap providers bootstrap, from the provider's own
    /// board.conf key (`DEBIAN_SUITE` / `UBUNTU_SUITE`).
    pub suite: Option<String>,
    /// Mirror they bootstrap from (`DEBIAN_MIRROR` / `UBUNTU_MIRROR`).
    pub mirror: Option<String>,
    /// ROOTFS_SECOND_STAGE; absent → chroot (build-time, needs qemu-user).
    pub second_stage: SecondStage,
    pub alpine_branch: Option<String>, // ALPINE_BRANCH; alpine provider, None → "v3.24"
    pub alpine_mirror: Option<String>, // ALPINE_MIRROR; alpine provider, None → dl-cdn
    pub alpine_repos: Option<String>,  // ALPINE_REPOS; alpine provider, None → "main community"
    pub fedora_release: Option<String>, // FEDORA_RELEASE; fedora provider, None → "44"
    pub fedora_mirror: Option<String>, // FEDORA_MIRROR; fedora provider, None → dl.fedoraproject.org/pub
    /// BUILDROOT_DEFCONFIG; buildroot provider.  A file in
    /// `boards/<board>/` if one is there, otherwise a name in
    /// buildroot's own `configs/` -- same board-first lookup
    /// KERNEL_CONFIG_FRAGMENTS uses.
    pub buildroot_defconfig: Option<String>,
    pub buildroot_repo: Option<String>, // BUILDROOT_REPO; None → upstream git
    pub buildroot_tag: Option<String>,  // BUILDROOT_TAG; None → "master" (warned as unpinned)

    // OpenWrt (openwrt provider).  Release, target and subtarget name one
    // immutable directory on the download server, and the profile names one
    // device in it; all four are required so an image is reproducible from
    // board.conf alone.
    pub openwrt_release: Option<String>,   // OPENWRT_RELEASE, e.g. "25.12.5"
    pub openwrt_target: Option<String>,    // OPENWRT_TARGET, e.g. "sifiveu"
    pub openwrt_subtarget: Option<String>, // OPENWRT_SUBTARGET, e.g. "generic"
    pub openwrt_profile: Option<String>,   // OPENWRT_PROFILE, e.g. "sifive_unmatched"
    pub openwrt_mirror: Option<String>,    // OPENWRT_MIRROR; None → downloads.openwrt.org
    /// OPENWRT_SHA256: the ImageBuilder tarball's hash as the board saw it.
    /// The published `sha256sums` is fetched over the same connection as the
    /// tarball, so it proves the download arrived intact and nothing more;
    /// this is the value a mirror cannot talk its way out of.
    pub openwrt_sha256: Option<String>,

    // OpenSBI
    pub opensbi_repo: Option<String>,
    pub opensbi_tag: Option<String>,
    pub opensbi_platform: Option<String>,
    pub opensbi_fw_type: Option<String>, // dynamic (default) | jump | payload
    pub opensbi_make_flags: Option<String>, // extra make args

    // U-Boot
    pub u_boot_repo: Option<String>,
    pub u_boot_tag: Option<String>,
    pub u_boot_defconfig: Option<String>,
    pub u_boot_make_flags: Option<String>, // extra make args

    // GRUB (BIOS/EFI bootloader via grub-mkimage)
    pub grub_platforms: Option<String>, // e.g. "pc"
    pub grub_modules: Option<String>,   // extra modules to embed in core.img

    // SYSLINUX (BIOS bootloader)
    pub syslinux_repo: Option<String>,
    pub syslinux_tag: Option<String>,

    // ARM Trusted Firmware-A — BL31 for Rockchip / Amlogic SoCs
    pub tfa_repo: Option<String>,
    pub tfa_tag: Option<String>,
    pub tfa_plat: Option<String>,

    // Rockchip closed-source blob repo (DDR init etc., pre-built)
    pub rkbin_repo: Option<String>,
    pub rkbin_tag: Option<String>,
    pub rkbin_ddr: Option<String>, // glob pattern for the DDR init blob

    // Amlogic boot-FIP packaging repo (vendor BL2/BL30/BL301 + tools)
    pub fip_repo: Option<String>,
    pub fip_tag: Option<String>,

    /// Ordered bootloader pipeline (`BOOT_PIPELINE` array).
    /// `None` (key absent) → DEFAULT_PIPELINE (`opensbi uboot syslinux grub`);
    /// `Some(vec![])` (explicit `()`) → no stages.
    pub boot_pipeline: Option<Vec<String>>,

    // Firmware overlay
    pub firmware_repo: Option<String>,
    pub firmware_tag: Option<String>,     // FIRMWARE_TAG; falls back to TAG
    pub firmware_overlay: Option<String>, // path inside firmware repo, contents -> /lib/firmware
    pub firmware_dirs: Vec<String>,       // dirs inside firmware repo, path preserved

    // Kernel
    pub kernel_repo: String,
    pub kernel_tag: String,
    pub kernel_defconfig: String,
    /// Names looked up as boards/<board>/kernel-config/<name>, falling back
    /// to defaults/kernel-config/<name>.  Appended to .config in listed
    /// order after the defconfig, and every line is checked afterwards.
    pub kernel_config_fragments: Vec<String>,
    pub kernel_dtb_glob: Option<String>,

    pub dracut_modules: Option<String>,
    /// Files (under the target rootfs) to bake into the initramfs via
    /// `dracut --install`.  Use for firmware that early drivers need
    /// before the real rootfs is mounted (e.g. K1's esos.elf for the
    /// rcpu-driven SCMI clock controller; without it PWM / clock
    /// drivers can hang waiting for the rcpu firmware).
    pub initramfs_install: Vec<String>,

    // Boot configuration
    pub root_dev: Option<String>,
    pub console: Option<String>,
    pub hostname: String,
    pub serial_tty: Option<String>,
    pub serial_baud: Option<String>,
    pub kernel_name: Option<String>,
    pub ramdisk_name: Option<String>,
    pub loglevel: Option<String>,

    /// BOOT_EXTLINUX: this board boots through an extlinux.conf, so `assemble`
    /// writes one instead of the board repeating the same file in a hook.
    pub extlinux: bool,
    /// BOOT_APPEND: kernel arguments beyond the ones every extlinux board
    /// states identically (root, rw, rootwait, rootfstype, console).
    pub append: Option<String>,
    /// BOOT_DTB_NAME: the device tree to boot, when BOARD_DTB_GLOB names more
    /// than one file and the board has to say which.
    pub dtb_name: Option<String>,

    /// ISA_STRICT: fail the build when a binary uses an ISA extension this
    /// board does not have.  On by default -- such a binary faults on the
    /// hardware, so shipping it is never the answer.  `ISA_STRICT="false"`
    /// while a board is being brought up and its stage3 residue is known.
    pub isa_strict: bool,

    pub services: Vec<String>, // e.g. ["sshd:default", "metalog:default"]
    pub build_steps: Vec<String>,

    // Per-package CFLAGS workarounds
    pub workaround_pkgs: Vec<String>,
    pub workaround_cflags: Vec<String>,

    pub image_name: Option<String>,
    pub compression: Option<String>, // xz (default) | gz | none

    /// Free-form labels from `TAGS=(...)` in board.conf (e.g.
    /// `["testing", "wip"]`).  Surfaced in `board list` / `status`.
    pub tags: Vec<String>,
    /// Free-form note from `DESCRIPTION=` in board.conf.
    pub description: Option<String>,
}

/// The steps `image build` runs for a board that does not list its own.
/// A board.conf states BUILD_STEPS only to depart from this order.
pub const DEFAULT_BUILD_STEPS: [&str; 6] = [
    "deps",
    "checkout",
    "bootloader",
    "kernel",
    "assemble",
    "pack",
];

impl BoardConfig {
    /// The steps this board builds through: its own list, or the default.
    pub fn effective_build_steps(&self) -> Vec<&str> {
        if self.build_steps.is_empty() {
            DEFAULT_BUILD_STEPS.to_vec()
        } else {
            self.build_steps.iter().map(String::as_str).collect()
        }
    }

    /// Derive the CHOST triple from the arch (e.g. "i586-pc-linux-gnu", "riscv64-unknown-linux-gnu").
    /// Uses explicit CHOST from board.conf if set, otherwise derives from arch.
    pub fn chost(&self) -> String {
        if let Some(ref chost) = self.chost_override {
            return chost.clone();
        }
        crate::stage::chost_for_arch(&self.arch)
            .unwrap_or_else(|_| format!("{}-unknown-linux-gnu", self.arch))
    }

    /// Effective CFLAGS (board-specific or arch default).
    pub fn effective_cflags(&self) -> String {
        self.cflags
            .clone()
            .unwrap_or_else(|| crate::stage::default_cflags(&self.arch).to_string())
    }

    /// Effective firmware checkout ref: FIRMWARE_TAG, else the U-Boot tag
    /// (vendor SDKs cut both from one release), else "main".
    pub fn effective_firmware_tag(&self) -> String {
        self.firmware_tag
            .clone()
            .or_else(|| self.u_boot_tag.clone())
            .unwrap_or_else(|| "main".to_string())
    }
}

/// Load a board configuration from `<boards_root>/<name>/board.conf`.
pub fn load(boards_root: &Utf8Path, name: &str) -> Result<BoardConfig> {
    let path = boards_root.join(name).join("board.conf");
    let content =
        std::fs::read_to_string(&path).map_err(|e| Error::BoardNotFound(format!("{path}: {e}")))?;

    // An include carries what several boards repeat -- an SoC's toolchain and
    // boot pipeline, or a role like "capture box".  Their lines go in front of
    // the board's, in the order listed, and the parser takes the last
    // assignment: the board wins any key it mentions, without a guard around
    // every shared default.
    //
    // An include cannot itself include.  One level keeps "where did this value
    // come from" answerable, and nothing here needs more.
    let mut merged = String::new();
    for name in includes_of(&content) {
        let include = boards_root.join("include").join(format!("{name}.conf"));
        let text = std::fs::read_to_string(&include)
            .map_err(|e| Error::BoardNotFound(format!("{include}: {e}")))?;
        merged.push_str(&text);
        merged.push('\n');
    }
    merged.push_str(&content);
    let board = parse(name, &path, &merged)?;
    check_cflags(&board, &path)?;
    Ok(board)
}

/// The `INCLUDE` list a board declares, read before parsing because it decides
/// what the parse input is.  Space separated, in priority order.
fn includes_of(content: &str) -> Vec<String> {
    content
        .lines()
        .find_map(|line| {
            let value = line.trim().strip_prefix("INCLUDE=")?;
            Some(
                value
                    .trim()
                    .trim_matches(['"', '\''])
                    .split_whitespace()
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap_or_default()
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

    // Fail fast on typos: reject unknown pipeline stages at load time,
    // not hours later when the bootloader step runs.
    let boot_pipeline = arrays.get("BOOT_PIPELINE").cloned();
    if let Some(stages) = &boot_pipeline {
        for s in stages {
            if !crate::bootloader::STAGES.contains(&s.as_str()) {
                return Err(Error::BoardConfigParse {
                    file: path.to_string(),
                    msg: format!(
                        "unknown BOOT_PIPELINE stage '{s}' (known: {})",
                        crate::bootloader::STAGES.join(" ")
                    ),
                });
            }
        }
    }

    let rootfs_provider = match kv.get("ROOTFS_PROVIDER") {
        Some(v) => RootfsProvider::parse(v).ok_or_else(|| Error::BoardConfigParse {
            file: path.to_string(),
            msg: format!(
                "unknown ROOTFS_PROVIDER '{v}' \
                 (valid: gentoo, debian, ubuntu, alpine, fedora, buildroot, openwrt, none)"
            ),
        })?,
        None => RootfsProvider::default(),
    };
    let deb = rootfs_provider.debootstrap();
    let second_stage = match kv.get("ROOTFS_SECOND_STAGE") {
        Some(v) if deb.is_none() => {
            return Err(Error::BoardConfigParse {
                file: path.to_string(),
                msg: format!(
                    "ROOTFS_SECOND_STAGE '{v}' only applies to a debootstrap provider \
                     (debian, ubuntu)"
                ),
            })
        }
        Some(v) => SecondStage::parse(v).ok_or_else(|| Error::BoardConfigParse {
            file: path.to_string(),
            msg: format!("unknown ROOTFS_SECOND_STAGE '{v}' (valid: chroot, first-boot)"),
        })?,
        None => SecondStage::default(),
    };

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
        rootfs_provider,
        // Each debootstrap provider reads its own key names, so a board
        // says UBUNTU_SUITE or DEBIAN_SUITE and never both.
        suite: deb.and_then(|d| kv.get(d.suite_key).cloned()),
        mirror: deb.and_then(|d| kv.get(d.mirror_key).cloned()),
        second_stage,
        alpine_branch: kv.get("ALPINE_BRANCH").cloned(),
        alpine_mirror: kv.get("ALPINE_MIRROR").cloned(),
        alpine_repos: kv.get("ALPINE_REPOS").cloned(),
        fedora_release: kv.get("FEDORA_RELEASE").cloned(),
        fedora_mirror: kv.get("FEDORA_MIRROR").cloned(),
        buildroot_defconfig: kv.get("BUILDROOT_DEFCONFIG").cloned(),
        buildroot_repo: kv.get("BUILDROOT_REPO").cloned(),
        // No TAG fallback, for the reason tfa/rkbin/fip have none: TAG
        // names a vendor-SDK ref that means nothing in buildroot's tree.
        buildroot_tag: kv.get("BUILDROOT_TAG").cloned(),

        openwrt_release: kv.get("OPENWRT_RELEASE").cloned(),
        openwrt_target: kv.get("OPENWRT_TARGET").cloned(),
        openwrt_subtarget: kv.get("OPENWRT_SUBTARGET").cloned(),
        openwrt_profile: kv.get("OPENWRT_PROFILE").cloned(),
        openwrt_mirror: kv.get("OPENWRT_MIRROR").cloned(),
        openwrt_sha256: kv.get("OPENWRT_SHA256").cloned(),

        opensbi_repo: kv.get("OPENSBI_REPO").cloned(),
        opensbi_tag: kv.get("OPENSBI_TAG").cloned(),
        opensbi_platform: kv.get("OPENSBI_PLATFORM").cloned(),
        opensbi_fw_type: kv.get("OPENSBI_FW_TYPE").cloned(),
        opensbi_make_flags: kv.get("OPENSBI_MAKE_FLAGS").cloned(),

        u_boot_repo: kv.get("U_BOOT_REPO").cloned(),
        u_boot_tag: kv.get("U_BOOT_TAG").or_else(|| kv.get("TAG")).cloned(),
        u_boot_defconfig: kv.get("U_BOOT_DEFCONFIG").cloned(),
        u_boot_make_flags: kv.get("U_BOOT_MAKE_FLAGS").cloned(),

        grub_platforms: kv.get("GRUB_PLATFORMS").cloned(),
        grub_modules: kv.get("GRUB_MODULES").cloned(),

        syslinux_repo: kv.get("SYSLINUX_REPO").cloned(),
        syslinux_tag: kv.get("SYSLINUX_TAG").cloned(),

        // No TAG fallback for tfa/rkbin/fip tags: TAG names a vendor-SDK
        // ref that never exists in these third-party repos.  The clone
        // sites default to "master".
        tfa_repo: kv.get("TFA_REPO").cloned(),
        tfa_tag: kv.get("TFA_TAG").cloned(),
        tfa_plat: kv.get("TFA_PLAT").cloned(),

        rkbin_repo: kv.get("RKBIN_REPO").cloned(),
        rkbin_tag: kv.get("RKBIN_TAG").cloned(),
        rkbin_ddr: kv.get("RKBIN_DDR").cloned(),

        fip_repo: kv.get("FIP_REPO").cloned(),
        fip_tag: kv.get("FIP_TAG").cloned(),

        boot_pipeline,

        firmware_repo: kv.get("FIRMWARE_REPO").cloned(),
        firmware_tag: kv.get("FIRMWARE_TAG").or_else(|| kv.get("TAG")).cloned(),
        firmware_overlay: kv.get("BOARD_FIRMWARE_OVERLAY").cloned(),
        firmware_dirs: arrays.get("FIRMWARE_DIRS").cloned().unwrap_or_default(),

        kernel_repo: req!("KERNEL_REPO"),
        kernel_tag: kv
            .get("KERNEL_TAG")
            .or_else(|| kv.get("TAG"))
            .cloned()
            .unwrap_or(tag.clone()),
        includes: kv
            .get("INCLUDE")
            .map(|v| v.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default(),
        kernel_defconfig: req!("KERNEL_DEFCONFIG"),
        kernel_config_fragments: kv
            .get("KERNEL_CONFIG_FRAGMENTS")
            .map(|v| v.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default(),
        kernel_dtb_glob: kv.get("BOARD_DTB_GLOB").cloned(),

        dracut_modules: kv.get("DRACUT_MODULES").cloned(),
        initramfs_install: arrays
            .get("INITRAMFS_INSTALL")
            .cloned()
            .or_else(|| {
                kv.get("INITRAMFS_INSTALL")
                    .map(|s| s.split_whitespace().map(String::from).collect())
            })
            .unwrap_or_default(),

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

        extlinux: kv
            .get("BOOT_EXTLINUX")
            .map(|v| v == "true" || v == "yes" || v == "1")
            .unwrap_or(false),
        append: kv.get("BOOT_APPEND").cloned(),
        dtb_name: kv.get("BOOT_DTB_NAME").cloned(),
        isa_strict: kv
            .get("ISA_STRICT")
            .map(|v| !(v == "false" || v == "no" || v == "0"))
            .unwrap_or(true),

        services: arrays.get("BOOT_SERVICES").cloned().unwrap_or_default(),
        build_steps: arrays.get("BUILD_STEPS").cloned().unwrap_or_default(),

        workaround_pkgs: arrays.get("WORKAROUND_PKGS").cloned().unwrap_or_default(),
        workaround_cflags: arrays.get("WORKAROUND_CFLAGS").cloned().unwrap_or_default(),

        image_name: kv.get("IMAGE_NAME").cloned(),
        compression: kv.get("COMPRESSION").cloned(),
        tags: arrays
            .get("TAGS")
            .cloned()
            .or_else(|| {
                kv.get("TAGS")
                    .map(|s| s.split_whitespace().map(String::from).collect())
            })
            .unwrap_or_default(),
        description: kv.get("DESCRIPTION").cloned(),
    })
}

/// Refuse a board whose CFLAGS cannot name a binary-package cache, before any
/// of it is built.  The key is what keeps boards from sharing binaries they
/// cannot run, so a flag set that would break the key is a configuration
/// error, not something to discover from a fault on the hardware.
fn check_cflags(board: &BoardConfig, path: &Utf8Path) -> Result<()> {
    crate::cflags::check(&board.effective_cflags()).map_err(|msg| Error::BoardConfigParse {
        file: path.to_string(),
        msg: format!("BOARD_CFLAGS: {msg}"),
    })?;

    // The two arrays are read pairwise, and `zip` stops at the shorter one, so
    // a board that lists three packages and two flag sets loses the third
    // without a word -- and the package is then built with the board's own
    // flags, which is the case the workaround existed to avoid.
    if board.workaround_pkgs.len() != board.workaround_cflags.len() {
        return Err(Error::BoardConfigParse {
            file: path.to_string(),
            msg: format!(
                "WORKAROUND_PKGS has {} entries and WORKAROUND_CFLAGS has {}; \
                 they are read pairwise",
                board.workaround_pkgs.len(),
                board.workaround_cflags.len()
            ),
        });
    }

    for flags in &board.workaround_cflags {
        crate::cflags::check(flags).map_err(|msg| Error::BoardConfigParse {
            file: path.to_string(),
            msg: format!("WORKAROUND_CFLAGS: {msg}"),
        })?;
    }
    Ok(())
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
