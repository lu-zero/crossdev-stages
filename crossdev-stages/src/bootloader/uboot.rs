use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::Result;

/// Clone the U-Boot source tree into /build/u-boot.
/// No-op if the board has no u_boot_repo configured.
pub fn clone(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    if let (Some(repo), Some(tag)) = (&board.u_boot_repo, &board.u_boot_tag) {
        runner.run(&format!(
            "git clone --depth=1 --branch {tag} {repo} /build/u-boot"
        ))?;
    }
    Ok(())
}

/// Build U-Boot: `make defconfig && make -j$(nproc)`
/// No-op if the board has no u_boot_defconfig configured.
pub fn build(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    if let Some(defconfig) = &board.u_boot_defconfig {
        let karch = board.kernel_arch.as_deref().ok_or_else(|| {
            crate::error::Error::BoardConfigParse {
                file: board.name.clone(),
                msg: "KERNEL_ARCH required for bootloader build".into(),
            }
        })?;
        runner.run(&format!(
            "make -C /build/u-boot ARCH={karch} CROSS_COMPILE={cc} {defconfig} && \
             make -C /build/u-boot ARCH={karch} CROSS_COMPILE={cc} -j$(nproc)",
            cc = board.cross_compile,
        ))?;
    }
    Ok(())
}
