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

/// Build OpenSBI with firmware type and extra flags from board.conf.
///
/// Reads:
/// - `OPENSBI_FW_TYPE`: "dynamic" (default), "jump", or "payload"
/// - `OPENSBI_MAKE_FLAGS`: extra make arguments (e.g. "LLVM=1 PLATFORM_DEFCONFIG=defconfig")
///
/// No-op if the board has no opensbi_platform configured.
pub fn build(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    if let (Some(platform), Some(_repo)) = (&board.opensbi_platform, &board.opensbi_repo) {
        let fw_flag = match board.opensbi_fw_type.as_deref() {
            Some("jump") => "FW_JUMP=y",
            Some("payload") => "FW_PAYLOAD=y",
            _ => "",
        };
        let extra = board.opensbi_make_flags.as_deref().unwrap_or("");
        runner.run(&format!(
            "make -C /build/opensbi PLATFORM={platform} \
             CROSS_COMPILE={cc} {fw_flag} {extra} -j$(nproc)",
            cc = board.cross_compile,
        ))?;
    }
    Ok(())
}
