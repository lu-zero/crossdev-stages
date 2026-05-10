//! Rockchip closed-source blob repo (`rockchip-linux/rkbin`).
//!
//! Ships pre-built binaries — clone-only, no build step.  `RKBIN_DDR` is
//! a glob pattern that often matches multiple versioned blobs; the newest
//! non-eyescan one is picked at use time via shell `$(...)` expansion in
//! [`exports()`].  U-Boot consumes the resolved path via `ROCKCHIP_TPL=`.

use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::Result;

pub fn clone(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    if let Some(repo) = &board.rkbin_repo {
        let tag = board.rkbin_tag.as_deref().unwrap_or("master");
        crate::source_cache::cached_clone(runner, repo, tag, "/build/rkbin", "rkbin")?;
    }
    Ok(())
}

pub fn build(runner: &SandboxRunner, board: &BoardConfig, _env: &[String]) -> Result<()> {
    // Pre-built; nothing to compile.  But fail fast if the DDR glob
    // resolves to nothing — otherwise an empty ROCKCHIP_TPL surfaces much
    // later as an obscure missing-blob error inside U-Boot's binman.
    if let (Some(_), Some(glob)) = (&board.rkbin_repo, &board.rkbin_ddr) {
        runner.run(&format!(
            "blob=$(ls /build/rkbin/{glob} 2>/dev/null | grep -v eyescan | sort -V | tail -1); \
             [ -n \"$blob\" ] || {{ echo \"error: no DDR blob matching {glob}\" >&2; exit 1; }}; \
             echo \"Using DDR blob: $blob\""
        ))?;
    }
    Ok(())
}

pub fn exports(board: &BoardConfig) -> Vec<String> {
    if let (Some(_), Some(glob)) = (&board.rkbin_repo, &board.rkbin_ddr) {
        vec![format!(
            "ROCKCHIP_TPL=$(ls /build/rkbin/{glob} 2>/dev/null | grep -v eyescan | sort -V | tail -1)"
        )]
    } else {
        Vec::new()
    }
}
