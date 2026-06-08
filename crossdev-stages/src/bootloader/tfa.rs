//! ARM Trusted Firmware-A: BL31 (EL3 secure monitor).
//!
//! Required by Rockchip (RK3588 etc.) and Amlogic (GXBB, G12A) SoCs.
//! U-Boot consumes the BL31 binary via the `BL31=` make var, exported by
//! [`exports()`] for the next pipeline stage to inherit.

use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::Result;

pub fn clone(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    if let Some(repo) = &board.tfa_repo {
        let tag = board.tfa_tag.as_deref().unwrap_or("master");
        crate::source_cache::cached_clone(runner, repo, tag, "/build/tfa", "tfa")?;
    }
    Ok(())
}

pub fn build(runner: &SandboxRunner, board: &BoardConfig, _env: &[String]) -> Result<()> {
    if let (Some(_repo), Some(plat)) = (&board.tfa_repo, &board.tfa_plat) {
        runner.run(&format!(
            "make -C /build/tfa PLAT={plat} CROSS_COMPILE={cc} bl31 -j$(nproc)",
            cc = board.cross_compile,
        ))?;
    }
    Ok(())
}

pub fn exports(board: &BoardConfig) -> Vec<String> {
    if let (Some(_), Some(plat)) = (&board.tfa_repo, &board.tfa_plat) {
        vec![format!("BL31=/build/tfa/build/{plat}/release/bl31/bl31.elf")]
    } else {
        Vec::new()
    }
}
