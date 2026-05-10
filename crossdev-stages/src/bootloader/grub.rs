//! GRUB: BIOS/EFI bootloader assembled with grub-mkimage.
//!
//! No source clone — the sandbox host's grub-mkimage (installed via
//! sandbox-packages.txt) is the tool, and the i386-pc modules come from the
//! crossdev prefix (`/usr/{chost}/usr/lib/grub/i386-pc/`), compiled with the
//! cross-compiler via `crossdev --ex-pkg sys-boot/grub`.  Both installs come
//! from the same portage package version, so the module ABI matches.

use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::Result;

/// Nothing to clone: grub-mkimage and the platform modules are installed
/// packages, not a source checkout.
pub fn clone(_runner: &SandboxRunner, _board: &BoardConfig) -> Result<()> {
    Ok(())
}

/// Build a GRUB i386-pc core image using grub-mkimage.
///
/// `boot.img` is staged to `/build/grub-boot.img` so the genimage exec-post
/// can reference it without knowing the source path.
pub fn build(runner: &SandboxRunner, board: &BoardConfig, _env: &[String]) -> Result<()> {
    let Some(_) = &board.grub_platforms else {
        return Ok(());
    };

    let chost = board.chost();
    let mods = format!("/usr/{chost}/usr/lib/grub/i386-pc");

    let default_modules = "biosdisk part_msdos fat ext2 normal boot linux \
                           configfile echo search search_label search_fs_uuid";
    let modules = board.grub_modules.as_deref().unwrap_or(default_modules);

    runner.run(&format!(
        "grub-mkimage -O i386-pc \
             -d {mods} \
             -o /build/grub-core.img \
             -p '(hd0,msdos1)/grub' \
             {modules} && \
         cp {mods}/boot.img /build/grub-boot.img"
    ))
}

/// GRUB's outputs (grub-boot.img, grub-core.img) are consumed at pack time
/// (genimage exec-post), not by later pipeline stages.
pub fn exports(_board: &BoardConfig) -> Vec<String> {
    Vec::new()
}
