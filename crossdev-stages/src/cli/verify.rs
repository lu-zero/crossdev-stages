use camino::Utf8Path;

use crate::error::Result;
use crate::{board, isa, sandbox::Sandbox, workspace::Workspace};

/// Check the binaries a board produced against the ISA its CFLAGS promise.
///
/// Runs against what is already on disk -- the build's image tree if there is
/// one, otherwise the shared target stage -- so it costs a scan, not a build.
pub fn run(
    ws: &Workspace,
    boards_root: &Utf8Path,
    board_name: &str,
    sandbox: Option<&str>,
    strict: bool,
) -> Result<()> {
    let board_cfg = board::load(boards_root, board_name)?;

    let sb = Sandbox::open(ws.resolve_sandbox(sandbox)?)?;
    let tgt = crate::target::Target::open(ws.resolve_target_for_arch(
        None,
        &board_cfg.arch,
        board_cfg.rootfs_provider.name(),
    )?)?;

    let build_dir = ws.builds_dir().join(board_name);
    let image_root = build_dir.join("gen/root");

    // The board's own runner, so the toolchain that answers "what ISA do you
    // emit" is the one that built these binaries.
    let base = sb
        .runner_for_board(ws, &board_cfg.arch, &board_cfg)?
        .with_target(&tgt.dir);
    let (runner, root) = if image_root.is_dir() {
        (
            base.with_build(&build_dir, boards_root.parent().unwrap_or(boards_root)),
            "/build/gen/root",
        )
    } else {
        (base, "/target")
    };

    // Every architecture: can the image load what it ships?
    let abi = crate::abi::verify(&runner, root, &format!("{}readelf", board_cfg.cross_compile))?;
    let mut fatal = crate::abi::print(&abi);

    if isa::applies(&board_cfg) {
        let report = isa::verify(&runner, &board_cfg, root)?;
        fatal |= isa::print(&report, Utf8Path::new(root));
    }

    if fatal && strict {
        return Err(crate::error::Error::CommandFailed {
            code: 1,
            reason: "binaries use extensions this board does not have".into(),
        });
    }
    Ok(())
}
