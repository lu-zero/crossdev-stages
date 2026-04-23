//! rkbin ships pre-built binaries -- no build step, clone only. RKBIN_DDR
//! typically matches multiple versioned DDR blobs; we pick the newest
//! non-eyescan one at build time.

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

pub fn ddr_blob_expr(board: &BoardConfig) -> Option<String> {
    let glob = board.rkbin_ddr.as_deref()?;
    board.rkbin_repo.as_ref()?;
    Some(format!(
        "$(ls /build/rkbin/{glob} 2>/dev/null | grep -v eyescan | sort -V | tail -1)"
    ))
}
