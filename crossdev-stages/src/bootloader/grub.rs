use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::Result;

/// Build a GRUB i386-pc core image using grub-mkimage.
///
/// Strategy: the sandbox host's grub-mkimage (native arch binary, installed via
/// sandbox-packages.txt) is used as the tool.  i386-pc modules come from the
/// crossdev prefix (`/usr/{chost}/usr/lib/grub/i386-pc/`), compiled with the
/// i586 cross-compiler via `crossdev --ex-pkg sys-boot/grub`.  Both installs
/// come from the same portage package version, so the module ABI matches.
///
/// `boot.img` is staged to `/build/grub-boot.img` so the genimage exec-post
/// can reference it without knowing the source path.
pub fn build(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
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
