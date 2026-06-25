use camino::Utf8Path;

use crate::error::Result;
use crate::workspace::Workspace;
use crate::{board, error, sandbox, stage, target};

/// Ensure the sandbox exists and is prepared (no cross-toolchain setup).
/// Auto-creates a sandbox from the host arch stage3 if none is found.
pub async fn ensure_sandbox(
    ws: &Workspace,
    sandbox_name: Option<&str>,
    defaults_root: &Utf8Path,
    mirror: Option<&str>,
) -> Result<sandbox::Sandbox> {
    let sd = match ws.resolve_sandbox(sandbox_name) {
        Ok(p) => p,
        Err(_) => {
            let host_arch = std::env::consts::ARCH;
            tracing::info!("No sandbox found, creating one for {host_arch}…");
            let source_stage = stage::fetch(&ws.stages_dir(), host_arch, mirror).await?;
            let name = format!(
                "{host_arch}-{}",
                chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
            );
            sandbox::Sandbox::create(ws, &name, host_arch, &source_stage)?;
            ws.resolve_sandbox(None)?
        }
    };
    let sb = sandbox::Sandbox::open(sd)?;
    sb.prepare(mirror, defaults_root, false)?;
    Ok(sb)
}

/// Ensure the sandbox exists, is prepared, and has crossdev for `arch`.
pub async fn ensure_crossdev(
    ws: &Workspace,
    sandbox_name: Option<&str>,
    arch: &str,
    board_cfg: &board::BoardConfig,
    defaults_root: &Utf8Path,
    mirror: Option<&str>,
    gcc_version: Option<&str>,
) -> Result<sandbox::Sandbox> {
    let sb = ensure_sandbox(ws, sandbox_name, defaults_root, mirror).await?;
    sb.setup_crossdev(ws, arch, board_cfg, gcc_version)?;
    Ok(sb)
}

/// Ensure the target exists (fetching + unpacking a stage3 if needed) and
/// that the sandbox has crossdev set up for its arch.  Returns (Target, Sandbox).
pub async fn ensure_target(
    ws: &Workspace,
    target_name: Option<&str>,
    arch_override: Option<&str>,
    sandbox_name: Option<&str>,
    defaults_root: &Utf8Path,
    mirror: Option<&str>,
) -> Result<(target::Target, sandbox::Sandbox)> {
    // If --arch is given, filter targets by that arch so we don't fall back to
    // the most-recently-modified target of a foreign arch.  This entry point
    // seeds a target from a stage3, so the provider it may adopt is Gentoo's.
    let gentoo = crate::provider::RootfsProvider::Gentoo.name();
    let resolved = match arch_override {
        Some(a) => ws.resolve_target_for_arch(target_name, a, gentoo),
        None => ws.resolve_target(target_name),
    };
    let (tgt, resolved_arch) = match resolved {
        Ok(td) => {
            let tgt = target::Target::open(td)?;
            let arch = arch_override
                .map(String::from)
                .unwrap_or_else(|| tgt.arch.clone());
            (tgt, arch)
        }
        Err(_) => {
            let arch = arch_override.ok_or_else(|| {
                error::Error::TargetNotFound(
                    "target not found; specify --arch to create one".into(),
                )
            })?;
            let name = target_name.unwrap_or(&format!("{arch}-stage1")).to_string();
            tracing::info!("Target '{name}' not found, creating from stage3…");
            let source_stage = stage::fetch(&ws.stages_dir(), arch, mirror).await?;
            let tgt = target::Target::create(ws, &name, arch, &source_stage)?;
            (tgt, arch.to_string())
        }
    };
    let sb = ensure_crossdev(
        ws,
        sandbox_name,
        &resolved_arch,
        &default_board_config(&resolved_arch),
        defaults_root,
        mirror,
        None,
    )
    .await?;
    Ok((tgt, sb))
}

/// Build a minimal `BoardConfig` when no board is specified for crossdev setup.
pub fn default_board_config(arch: &str) -> board::BoardConfig {
    board::BoardConfig {
        name: arch.to_string(),
        arch: arch.to_string(),
        chost_override: None,
        cflags: None,
        ldflags: None,
        rustflags: None,
        gcc_version: None,
        cross_compile: format!(
            "{}-",
            crate::stage::chost_for_arch(arch)
                .unwrap_or_else(|_| format!("{arch}-unknown-linux-gnu"))
        ),
        kernel_arch: None,
        rootfs_provider: crate::provider::RootfsProvider::default(),
        debian_suite: None,
        debian_mirror: None,
        opensbi_repo: None,
        opensbi_tag: None,
        opensbi_platform: None,
        opensbi_fw_type: None,
        opensbi_make_flags: None,
        u_boot_repo: None,
        u_boot_tag: None,
        u_boot_defconfig: None,
        u_boot_make_flags: None,
        grub_platforms: None,
        grub_modules: None,
        syslinux_repo: None,
        syslinux_tag: None,
        tfa_repo: None,
        tfa_tag: None,
        tfa_plat: None,
        rkbin_repo: None,
        rkbin_tag: None,
        rkbin_ddr: None,
        fip_repo: None,
        fip_tag: None,
        boot_pipeline: None,
        firmware_repo: None,
        firmware_tag: None,
        firmware_overlay: None,
        firmware_dirs: vec![],
        kernel_repo: String::new(),
        kernel_tag: String::new(),
        kernel_defconfig: String::new(),
        includes: Vec::new(),
        kernel_dtb_glob: None,
        kernel_config_fragments: Vec::new(),
        dracut_modules: None,
        root_dev: None,
        console: None,
        hostname: "gentoo".into(),
        serial_tty: None,
        serial_baud: None,
        kernel_name: None,
        ramdisk_name: None,
        extlinux: false,
        append: None,
        dtb_name: None,
        isa_strict: true,
        loglevel: None,
        services: vec![],
        build_steps: vec![],
        workaround_pkgs: vec![],
        workaround_cflags: vec![],
        image_name: None,
        compression: None,
        tags: vec![],
        description: None,
    }
}
