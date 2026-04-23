use crate::{board, error, sandbox, stage, target};
use crate::workspace::Workspace;
use crate::error::Result;

/// Ensure the sandbox exists, is prepared, and has crossdev for `arch`.
/// Auto-creates a sandbox from the host arch stage3 if none is found.
pub async fn ensure_crossdev(
    ws: &Workspace,
    sandbox_name: Option<&str>,
    arch: &str,
    board_cfg: &board::BoardConfig,
    mirror: Option<&str>,
) -> Result<sandbox::Sandbox> {
    let sd = match ws.resolve_sandbox(sandbox_name) {
        Ok(p) => p,
        Err(_) => {
            let host_arch = std::env::consts::ARCH;
            tracing::info!("No sandbox found, creating one for {host_arch}…");
            let source_stage = stage::fetch(&ws.stages_dir(), host_arch, mirror).await?;
            let name =
                format!("{host_arch}-{}", chrono::Utc::now().format("%Y%m%dT%H%M%SZ"));
            sandbox::Sandbox::create(ws, &name, host_arch, &source_stage)?;
            ws.resolve_sandbox(None)?
        }
    };
    let sb = sandbox::Sandbox::open(sd)?;
    sb.prepare(mirror)?;
    sb.setup_crossdev(arch, board_cfg)?;
    Ok(sb)
}

/// Ensure the target exists (fetching + unpacking a stage3 if needed) and
/// that the sandbox has crossdev set up for its arch.  Returns (Target, Sandbox).
pub async fn ensure_target(
    ws: &Workspace,
    target_name: Option<&str>,
    arch_override: Option<&str>,
    sandbox_name: Option<&str>,
    mirror: Option<&str>,
) -> Result<(target::Target, sandbox::Sandbox)> {
    let (tgt, resolved_arch) = match ws.resolve_target(target_name) {
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
            let name = target_name
                .unwrap_or(&format!("{arch}-stage1"))
                .to_string();
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
        mirror,
    )
    .await?;
    Ok((tgt, sb))
}

/// Build a minimal `BoardConfig` when no board is specified for crossdev setup.
pub fn default_board_config(arch: &str) -> board::BoardConfig {
    board::BoardConfig {
        name: arch.to_string(),
        arch: arch.to_string(),
        cflags: None,
        ldflags: None,
        rustflags: None,
        cross_compile: format!("{arch}-unknown-linux-gnu-"),
        kernel_arch: None,
        opensbi_repo: None,
        opensbi_tag: None,
        opensbi_platform: None,
        opensbi_fw_type: None,
        opensbi_make_flags: None,
        u_boot_repo: None,
        u_boot_tag: None,
        u_boot_defconfig: None,
        u_boot_make_flags: None,
        tfa_repo: None,
        tfa_tag: None,
        tfa_plat: None,
        rkbin_repo: None,
        rkbin_tag: None,
        rkbin_ddr: None,
        firmware_repo: None,
        firmware_tag: None,
        firmware_overlay: None,
        host_firmware_paths: vec![],
        kernel_repo: String::new(),
        kernel_tag: String::new(),
        kernel_defconfig: String::new(),
        kernel_dtb_glob: None,
        dracut_modules: None,
        root_dev: None,
        console: None,
        hostname: "gentoo".into(),
        serial_tty: None,
        serial_baud: None,
        kernel_name: None,
        ramdisk_name: None,
        loglevel: None,
        services: vec![],
        build_steps: vec![],
        workaround_pkgs: vec![],
        workaround_cflags: vec![],
        image_name: None,
        compression: None,
        testing: false,
    }
}
