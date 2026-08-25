//! Amlogic boot-FIP packaging.
//!
//! Combines vendor-signed BL2/BL30/BL301 + prebuilt-or-built BL31 + DDR
//! firmware + our U-Boot into a SD-bootable `u-boot.bin.sd.bin` via the
//! repo's `build-fip.sh`.  Used by Amlogic SoCs (GXBB, G12A, SM1, ...).
//!
//! Runs AFTER U-Boot (consumes /build/u-boot/u-boot.bin).

use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::Result;

pub fn clone(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    if let Some(repo) = &board.fip_repo {
        let tag = board.fip_tag.as_deref().unwrap_or("master");
        crate::source_cache::cached_clone(runner, repo, tag, "/build/fip", "amlogic-fip")?;
    }
    Ok(())
}

pub fn build(runner: &SandboxRunner, board: &BoardConfig, _env: &[String]) -> Result<()> {
    if board.fip_repo.is_some() {
        // build-fip.sh only takes BL33; the remaining blobs come from the
        // repo's per-board directory.  When the tfa stage built a BL31,
        // substitute it for the shipped one before packing.
        let bl31 = match (&board.tfa_repo, &board.tfa_plat) {
            (Some(_), Some(plat)) => format!(
                "cp /build/tfa/build/{plat}/release/bl31.bin /build/fip/{name}/bl31.bin && ",
                name = board.name,
            ),
            _ => String::new(),
        };
        runner.run(&format!(
            "mkdir -p /build/u-boot-sd && \
             {bl31}cd /build/fip && \
             ./build-fip.sh {board} /build/u-boot/u-boot.bin /build/u-boot-sd/",
            board = board.name,
        ))?;
    }
    Ok(())
}

pub fn exports(_board: &BoardConfig) -> Vec<String> {
    Vec::new()
}
