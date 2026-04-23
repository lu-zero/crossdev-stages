use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::Result;

pub fn clone(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    if let (Some(repo), Some(tag)) = (&board.u_boot_repo, &board.u_boot_tag) {
        crate::source_cache::cached_clone(runner, repo, tag, "/build/u-boot", "u-boot")?;
    }
    Ok(())
}

/// U-Boot derives arch from defconfig and names aarch64 as `arm` (not `arm64`),
/// so forwarding Linux's `KERNEL_ARCH` as `ARCH=` breaks the build.
pub fn build(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    if let Some(defconfig) = &board.u_boot_defconfig {
        let extra = board.u_boot_make_flags.as_deref().unwrap_or("");
        let mut env = String::new();
        if let Some(bl31) = super::tfa::bl31_path(board) {
            env.push_str(&format!("BL31={bl31} "));
        }
        if let Some(ddr) = super::rkbin::ddr_blob_expr(board) {
            env.push_str(&format!("ROCKCHIP_TPL={ddr} "));
        }
        runner.run(&format!(
            "{env}make -C /build/u-boot CROSS_COMPILE={cc} {extra} {defconfig} && \
             {env}make -C /build/u-boot CROSS_COMPILE={cc} {extra} -j$(nproc)",
            cc = board.cross_compile,
        ))?;
    }
    Ok(())
}
