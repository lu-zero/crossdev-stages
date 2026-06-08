//! OpenSBI: RISC-V Supervisor Binary Interface.
//!
//! Vendor SDKs (k1, k230, ky-x1) clone an OpenSBI fork and build it as
//! `payload` (kernel embedded) or `dynamic` (runtime hand-off to U-Boot).

use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::Result;

pub fn clone(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    if let (Some(repo), Some(tag)) = (&board.opensbi_repo, &board.opensbi_tag) {
        crate::source_cache::cached_clone(runner, repo, tag, "/build/opensbi", "opensbi")?;
    }
    Ok(())
}

pub fn build(runner: &SandboxRunner, board: &BoardConfig, env: &[String]) -> Result<()> {
    if let (Some(platform), Some(_repo)) = (&board.opensbi_platform, &board.opensbi_repo) {
        let fw_flag = match board.opensbi_fw_type.as_deref() {
            Some("jump") => "FW_JUMP=y",
            Some("payload") => "FW_PAYLOAD=y",
            _ => "",
        };
        let extra = board.opensbi_make_flags.as_deref().unwrap_or("");
        let env_str = env.join(" ");
        runner.run(&format!(
            "{env_str} make -C /build/opensbi PLATFORM={platform} \
             CROSS_COMPILE={cc} {fw_flag} {extra} -j$(nproc)",
            cc = board.cross_compile,
        ))?;
    }
    Ok(())
}

/// OpenSBI's outputs are consumed at pack time (genimage), not by later
/// pipeline stages.
pub fn exports(_board: &BoardConfig) -> Vec<String> {
    Vec::new()
}
