//! U-Boot: universal bootloader.
//!
//! Typically the FINAL stage in a pipeline.  Consumes env contributions
//! from earlier stages — `BL31=` (from `tfa::exports`), `ROCKCHIP_TPL=`
//! (from `rkbin::exports`), etc. — without knowing about those stages
//! directly.

use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::Result;

pub fn clone(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    if let (Some(repo), Some(tag)) = (&board.u_boot_repo, &board.u_boot_tag) {
        crate::source_cache::cached_clone(runner, repo, tag, "/build/u-boot", "u-boot")?;
    }
    Ok(())
}

pub fn build(runner: &SandboxRunner, board: &BoardConfig, env: &[String]) -> Result<()> {
    if let Some(defconfig) = &board.u_boot_defconfig {
        let extra = board.u_boot_make_flags.as_deref().unwrap_or("");
        let env_str = env.join(" ");
        // U-Boot derives ARCH from the chosen defconfig.  Forwarding
        // Linux's KERNEL_ARCH (e.g. "arm64") breaks aarch64 builds because
        // U-Boot expects "arm".  Don't pass ARCH=.
        runner.run(&format!(
            "{env_str} make -C /build/u-boot CROSS_COMPILE={cc} {extra} {defconfig} && \
             {env_str} make -C /build/u-boot CROSS_COMPILE={cc} {extra} -j$(nproc)",
            cc = board.cross_compile,
        ))?;
    }
    Ok(())
}

/// U-Boot's outputs (u-boot.bin, u-boot.itb) are consumed at pack time.
pub fn exports(_board: &BoardConfig) -> Vec<String> {
    Vec::new()
}
