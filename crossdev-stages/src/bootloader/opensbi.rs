use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::Result;

/// Clone the OpenSBI source tree into /build/opensbi.
/// No-op if the board has no opensbi_repo configured.
pub fn clone(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    if let (Some(repo), Some(tag)) = (&board.opensbi_repo, &board.opensbi_tag) {
        runner.run(&format!(
            "git clone --depth=1 --branch {tag} {repo} /build/opensbi"
        ))?;
    }
    Ok(())
}

/// Build OpenSBI: `make -C /build/opensbi PLATFORM=... CROSS_COMPILE=... -j$(nproc)`
/// No-op if the board has no opensbi_platform configured.
pub fn build(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    if let (Some(platform), Some(_repo)) = (&board.opensbi_platform, &board.opensbi_repo) {
        runner.run(&format!(
            "make -C /build/opensbi PLATFORM={platform} \
             CROSS_COMPILE={cc} -j$(nproc)",
            cc = board.cross_compile,
        ))?;
    }
    Ok(())
}
