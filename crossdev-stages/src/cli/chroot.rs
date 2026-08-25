use camino::Utf8Path;

use crate::error::{Error, Result};
use crate::{board, workspace::Workspace};

/// Open a shell inside a board's own rootfs.
///
/// Prefers the built image tree, because that is the thing that goes on the
/// card; falls back to the shared target stage, which is the same filesystem
/// before the board's own steps ran.
pub fn run(
    ws: &Workspace,
    boards_root: &Utf8Path,
    board_name: &str,
    cmd: &[String],
) -> Result<()> {
    let board_cfg = board::load(boards_root, board_name)?;

    let image_root = ws.builds_dir().join(board_name).join("gen/root");
    let rootfs = if image_root.is_dir() {
        image_root
    } else {
        let dir = ws
            .resolve_target_for_arch(None, &board_cfg.arch, board_cfg.rootfs_provider.name())
            .map_err(|_| Error::CommandFailed {
                code: 1,
                reason: format!(
                    "{board_name} has neither a built image nor a {} target stage yet",
                    board_cfg.arch
                ),
            })?;
        dir
    };

    crate::chroot::run(&rootfs, &board_cfg.arch, cmd)
}
