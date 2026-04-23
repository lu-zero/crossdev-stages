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

pub fn build(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    if let (Some(_repo), Some(plat)) = (&board.tfa_repo, &board.tfa_plat) {
        runner.run(&format!(
            "make -C /build/tfa PLAT={plat} CROSS_COMPILE={cc} bl31 -j$(nproc)",
            cc = board.cross_compile,
        ))?;
    }
    Ok(())
}

/// Rockchip U-Boot consumes the `.elf` via `BL31=…`. Amlogic FIP tooling
/// consumes the sibling `.bin` at `.../release/bl31.bin` directly.
pub fn bl31_path(board: &BoardConfig) -> Option<String> {
    let plat = board.tfa_plat.as_deref()?;
    board.tfa_repo.as_ref()?;
    Some(format!("/build/tfa/build/{plat}/release/bl31/bl31.elf"))
}
