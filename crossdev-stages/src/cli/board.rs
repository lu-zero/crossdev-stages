use camino::Utf8Path;
use crate::board;
use crate::error::Result;
use crate::cli::BoardCmd;

pub fn run(boards_root: &Utf8Path, cmd: BoardCmd) -> Result<()> {
    match cmd {
        BoardCmd::List => {
            for b in board::list(boards_root)? {
                let tag = board::load(boards_root, &b)
                    .map(|c| if c.testing { " [TESTING]" } else { "" })
                    .unwrap_or("");
                println!("{b}{tag}");
            }
        }
        BoardCmd::Info { board: board_name } => {
            let board_cfg = board::load(boards_root, &board_name)?;
            println!("Board:          {}", board_cfg.name);
            println!("Arch:           {}", board_cfg.arch);
            println!("CHOST:          {}", board_cfg.chost());
            println!("CFLAGS:         {}", board_cfg.effective_cflags());
            println!("Cross-compile:  {}", board_cfg.cross_compile);
            if let Some(k) = &board_cfg.kernel_arch { println!("Kernel arch:    {k}"); }
            println!("Kernel repo:    {}", board_cfg.kernel_repo);
            println!("Kernel tag:     {}", board_cfg.kernel_tag);
            println!("Kernel defconf: {}", board_cfg.kernel_defconfig);
            if let Some(r) = &board_cfg.opensbi_repo { println!("OpenSBI repo:   {r}"); }
            if let Some(t) = &board_cfg.opensbi_tag { println!("OpenSBI tag:    {t}"); }
            if let Some(p) = &board_cfg.opensbi_platform { println!("OpenSBI plat:   {p}"); }
            if let Some(f) = &board_cfg.opensbi_fw_type { println!("OpenSBI fw:     {f}"); }
            if let Some(f) = &board_cfg.opensbi_make_flags { println!("OpenSBI flags:  {f}"); }
            if let Some(r) = &board_cfg.u_boot_repo { println!("U-Boot repo:    {r}"); }
            if let Some(t) = &board_cfg.u_boot_tag { println!("U-Boot tag:     {t}"); }
            if let Some(d) = &board_cfg.u_boot_defconfig { println!("U-Boot deconf:  {d}"); }
            if let Some(f) = &board_cfg.u_boot_make_flags { println!("U-Boot flags:   {f}"); }
            if let Some(r) = &board_cfg.tfa_repo { println!("TFA repo:       {r}"); }
            if let Some(t) = &board_cfg.tfa_tag { println!("TFA tag:        {t}"); }
            if let Some(p) = &board_cfg.tfa_plat { println!("TFA plat:       {p}"); }
            if let Some(r) = &board_cfg.rkbin_repo { println!("rkbin repo:     {r}"); }
            if let Some(g) = &board_cfg.rkbin_ddr { println!("rkbin DDR:      {g}"); }
            if let Some(r) = &board_cfg.fip_repo { println!("FIP repo:       {r}"); }
            if let Some(t) = &board_cfg.fip_tag { println!("FIP tag:        {t}"); }
            let pipeline = crate::bootloader::pipeline(&board_cfg);
            let default_marker = if board_cfg.boot_pipeline.is_empty() { " (default)" } else { "" };
            println!("Boot pipeline:  {}{default_marker}", pipeline.join(" "));
            if !board_cfg.build_steps.is_empty() {
                println!("Build steps:    {}", board_cfg.build_steps.join(" "));
            }
            if board_cfg.testing { println!("Testing:        yes"); }

            let board_dir = boards_root.join(&board_name);
            let steps = ["deps", "checkout", "bootloader", "kernel", "assemble", "pack"];
            let mut hooks = Vec::new();
            for s in &steps {
                if board_dir.join(format!("override-{s}.sh")).exists() {
                    hooks.push(format!("override-{s}.sh"));
                }
                if board_dir.join(format!("pre-{s}.sh")).exists() {
                    hooks.push(format!("pre-{s}.sh"));
                }
                if board_dir.join(format!("post-{s}.sh")).exists() {
                    hooks.push(format!("post-{s}.sh"));
                }
            }
            if !hooks.is_empty() {
                println!("Hooks:          {}", hooks.join(", "));
            }
        }
    }
    Ok(())
}
