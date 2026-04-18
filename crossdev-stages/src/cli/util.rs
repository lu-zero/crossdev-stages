use camino::Utf8Path;
use crossdev_stages::{board, error, sandbox, stage, target};
use crossdev_stages::workspace::Workspace;
use crossdev_stages::error::Result;

/// Ensure the sandbox exists, is prepared, and has crossdev for `arch`.
/// Auto-creates a sandbox from the host arch stage3 if none is found.
pub async fn ensure_crossdev(
    ws: &Workspace,
    sandbox_name: Option<&str>,
    arch: &str,
    board_cfg: &board::BoardConfig,
    mirror: Option<&str>,
    portage_overlay: Option<&Utf8Path>,
) -> Result<sandbox::Sandbox> {
    let sd = match ws.resolve_sandbox(sandbox_name) {
        Ok(p) => p,
        Err(_) => {
            let host_arch = std::env::consts::ARCH;
            tracing::info!("No sandbox found, creating one for {host_arch}…");
            let stage_file = stage::fetch(&ws.stages_dir(), host_arch, mirror).await?;
            let name =
                format!("{host_arch}-{}", chrono::Utc::now().format("%Y%m%dT%H%M%SZ"));
            sandbox::Sandbox::create(ws, &name, host_arch, &stage_file)?;
            ws.resolve_sandbox(None)?
        }
    };
    let sb = sandbox::Sandbox::open(sd)?;
    sb.prepare(mirror, portage_overlay)?;
    sb.setup_crossdev(arch, board_cfg, portage_overlay)?;
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
    portage_overlay: Option<&Utf8Path>,
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
                .map(String::from)
                .unwrap_or_else(|| format!("{arch}-stage1"));
            tracing::info!("Target '{name}' not found, creating from stage3…");
            let stage_file = stage::fetch(&ws.stages_dir(), arch, mirror).await?;
            let tgt = target::Target::create(ws, &name, arch, &stage_file)?;
            (tgt, arch.to_string())
        }
    };
    let sb = ensure_crossdev(
        ws,
        sandbox_name,
        &resolved_arch,
        &default_board_config(&resolved_arch),
        mirror,
        portage_overlay,
    )
    .await?;
    Ok((tgt, sb))
}

/// Build a minimal `BoardConfig` when no board is specified for crossdev setup.
pub fn default_board_config(arch: &str) -> board::BoardConfig {
    board::BoardConfig {
        name: arch.to_string(),
        arch: arch.to_string(),
        cross_compile: format!("{arch}-unknown-linux-gnu-"),
        hostname: "gentoo".into(),
        ..Default::default()
    }
}
