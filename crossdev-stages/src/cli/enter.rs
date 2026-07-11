use camino::Utf8Path;

use crate::error::Result;
use crate::{board, sandbox::Sandbox, workspace::Workspace};

/// Open a shell in the same container a board's build step runs in.
///
/// The point is that it is the *same* container: same rootfs, same /target,
/// /build, /scripts and /cache, same toolchain on PATH.  Debugging a failed
/// step by reconstructing the environment by hand gets the environment subtly
/// wrong, which is how an afternoon goes missing.
pub fn run(
    ws: &Workspace,
    boards_root: &Utf8Path,
    board_name: &str,
    sandbox: Option<&str>,
    cmd: &[String],
) -> Result<()> {
    let board_cfg = board::load(boards_root, board_name)?;
    let sb = Sandbox::open(ws.resolve_sandbox(sandbox)?)?;

    // The board's own runner: its keyed toolchain overlaid at /usr/<chost>,
    // so the shell sees exactly the compiler the build would use.
    let mut runner = sb
        .runner_for_board(ws, &board_cfg.arch, &board_cfg)?
        .with_cache(ws.base());

    // Both are optional: a board that has never been built still has a
    // toolchain worth poking at, and that is often exactly when you want one.
    if let Ok(dir) =
        ws.resolve_target_for_arch(None, &board_cfg.arch, board_cfg.rootfs_provider.name())
    {
        runner = runner.with_target(&dir);
    }
    let build_dir = ws.builds_dir().join(board_name);
    if build_dir.is_dir() {
        let project_root = boards_root.parent().unwrap_or(boards_root);
        runner = runner.with_build(&build_dir, project_root);
    }

    if cmd.is_empty() {
        println!(
            "{board_name}: {} in sandbox {}",
            board_cfg.chost(),
            sb.dir.file_name().unwrap_or_default()
        );
        runner.shell()
    } else {
        runner.run(&cmd.join(" "))
    }
}
