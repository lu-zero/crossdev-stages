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

pub fn build(_runner: &SandboxRunner, _board: &BoardConfig, _env: &[String]) -> Result<()> {
    Ok(()) // pre-built; nothing to compile
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
