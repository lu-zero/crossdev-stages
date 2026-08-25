//! U-Boot: universal bootloader.
//!
//! Typically the FINAL stage in a pipeline.  Consumes env contributions
//! from earlier stages — `BL31=` (from `tfa::exports`), `ROCKCHIP_TPL=`
//! (from `rkbin::exports`), etc. — without knowing about those stages
//! directly.

use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::{Error, Result};

pub fn clone(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    match (&board.u_boot_repo, &board.u_boot_tag) {
        (Some(repo), Some(tag)) => {
            crate::source_cache::cached_clone(runner, repo, tag, "/build/u-boot", "u-boot")
        }
        // A stage named in BOOT_PIPELINE with nothing configured is a no-op by
        // design: the default pipeline lists every stage and boards enable the
        // ones they need.  A repo with no ref is different -- the board meant
        // to use this stage, and skipping it silently produced an image with no
        // bootloader in it that still built and still passed CI.
        (Some(_), None) => Err(Error::BoardConfigParse {
            file: board.name.clone(),
            msg: "U_BOOT_REPO is set but U_BOOT_TAG is not, and neither is TAG".into(),
        }),
        (None, _) => Ok(()),
    }
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
