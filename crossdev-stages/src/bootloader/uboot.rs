use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::Result;

/// Clone U-Boot into /build/u-boot, using bare repo cache at /cache/sources/.
pub fn clone(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    if let (Some(repo), Some(tag)) = (&board.u_boot_repo, &board.u_boot_tag) {
        crate::source_cache::cached_clone(runner, repo, tag, "/build/u-boot", "u-boot")?;
    }
    Ok(())
}

/// Build U-Boot with extra flags from board.conf.
pub fn build(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    if let Some(defconfig) = &board.u_boot_defconfig {
        let karch =
            board
                .kernel_arch
                .as_deref()
                .ok_or_else(|| crate::error::Error::BoardConfigParse {
                    file: board.name.clone(),
                    msg: "KERNEL_ARCH required for bootloader build".into(),
                })?;
        let extra = board.u_boot_make_flags.as_deref().unwrap_or("");
        runner.run(&format!(
            "make -C /build/u-boot ARCH={karch} CROSS_COMPILE={cc} {extra} {defconfig} && \
             make -C /build/u-boot ARCH={karch} CROSS_COMPILE={cc} {extra} -j$(nproc)",
            cc = board.cross_compile,
        ))?;
    }
    Ok(())
}
