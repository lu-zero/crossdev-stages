use std::collections::HashMap;
use std::sync::LazyLock;

use camino::Utf8Path;

use crate::container::SandboxRunner;
use crate::error::Result;
use crate::stage::{default_cflags, gentoo_arch, llvm_target};

/// Parse a bash-style `KEY="value"` config file. `#` comments and blank lines
/// ignored. Quotes around values (either `"` or `'`) stripped.
pub fn parse_keyval(content: &str) -> HashMap<String, String> {
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            let v = v.trim().trim_matches('"').trim_matches('\'');
            Some((k.trim().to_string(), v.to_string()))
        })
        .collect()
}

static BUILD_CONFIG: LazyLock<HashMap<String, String>> =
    LazyLock::new(|| parse_keyval(include_str!("../config/build.conf")));

/// Return the configured GCC slot (defaults to "16" if missing).
/// Drives host + cross-sysroot `sys-devel/gcc:N` selection.
pub fn gcc_slot() -> &'static str {
    static SLOT: LazyLock<String> = LazyLock::new(|| {
        BUILD_CONFIG
            .get("GCC_SLOT")
            .cloned()
            .unwrap_or_else(|| "16".to_string())
    });
    SLOT.as_str()
}

/// Return the baseline `FEATURES` string applied via make.conf.
/// Kept out of the fragments tree so stage3's catalyst-written make.conf
/// content is preserved (set_make_conf_var only touches managed variables).
pub fn features_base() -> &'static str {
    static F: LazyLock<String> = LazyLock::new(|| {
        BUILD_CONFIG
            .get("FEATURES_BASE")
            .cloned()
            .unwrap_or_else(|| "parallel-install -merge-wait".to_string())
    });
    F.as_str()
}

/// Parameters for a Portage `make.conf` file.
pub struct MakeConf<'a> {
    pub arch: &'a str,
    pub chost: Option<&'a str>,
    pub cflags: Option<&'a str>,
    pub mirror: Option<&'a str>,
    pub binhost: Option<&'a str>,
}

impl<'a> MakeConf<'a> {
    /// Write `make.conf` into `portage_dir` (i.e. `/etc/portage` of a sandbox or sysroot).
    /// Updates variables in-place; preserves any existing content not managed here.
    pub fn write(&self, portage_dir: &Utf8Path) -> Result<()> {
        std::fs::create_dir_all(portage_dir)?;

        let make_conf = portage_dir.join("make.conf");
        if !make_conf.exists() {
            std::fs::write(&make_conf, "")?;
        }

        let (jobs, load) = parallelism();
        let garch = gentoo_arch(self.arch)?;
        let cflags = self.cflags.unwrap_or_else(|| default_cflags(self.arch));

        set_make_conf_var(&make_conf, "MAKEOPTS", &format!("-j{jobs} --load-average {load}"))?;
        set_make_conf_var(
            &make_conf,
            "EMERGE_DEFAULT_OPTS",
            &format!("--jobs={jobs} --load-average {load}"),
        )?;
        // FEATURES baseline from config/build.conf (getbinpkg appended below
        // if --binhost is set).
        set_make_conf_var(&make_conf, "FEATURES", features_base())?;
        // ~ARCH is mandatory — our toolchain (gcc:N, rust, clang-crossdev-wrappers)
        // is testing-only on all arches we support.
        set_make_conf_var(&make_conf, "ACCEPT_KEYWORDS", &format!("~{garch}"))?;
        set_make_conf_var(&make_conf, "PORT_LOGDIR", &format!("/var/log/portage/{garch}"))?;

        if let Some(chost) = self.chost {
            set_make_conf_var(&make_conf, "CHOST", chost)?;
            set_make_conf_var(&make_conf, "CFLAGS", cflags)?;
            set_make_conf_var(&make_conf, "CXXFLAGS", cflags)?;
            if let Some(llvm) = llvm_target(self.arch) {
                set_make_conf_var(&make_conf, "LLVM_TARGETS", llvm)?;
            }
        }

        if let Some(mirror) = self.mirror {
            set_make_conf_var(&make_conf, "GENTOO_MIRRORS", mirror)?;
        }

        if let Some(binhost) = self.binhost {
            set_make_conf_var(&make_conf, "PORTAGE_BINHOST", binhost)?;
            set_make_conf_var(
                &make_conf,
                "FEATURES",
                &format!("{} getbinpkg", features_base()),
            )?;
        }

        Ok(())
    }
}

/// Return (jobs, load-average) for MAKEOPTS / EMERGE_DEFAULT_OPTS.
///
/// Defaults to (nproc, nproc * 3 / 2) — full CPU saturation with a little
/// headroom for I/O-bound tasks. Override via `MAKEOPTS_JOBS` / `MAKEOPTS_LOAD`
/// in `config/build.conf` (integers, 0 or empty = auto).
fn parallelism() -> (usize, usize) {
    let n = num_cpus::get();
    let jobs = BUILD_CONFIG
        .get("MAKEOPTS_JOBS")
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(n);
    let load = BUILD_CONFIG
        .get("MAKEOPTS_LOAD")
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(n * 3 / 2);
    (jobs, load)
}

/// Static portage config fragments embedded from `crossdev-stages/config/portage/default/`.
/// Copied verbatim into the sandbox / cross-sysroot / target `/etc/portage/`
/// so policy tweaks (USE flags etc.) are diffs on real text files.
///
/// NOT included here:
/// - `make.conf` — managed in-place by [`MakeConf::write`] (preserves stage3's
///   catalyst-written content; only our managed vars are appended/replaced).
/// - `package.accept_keywords/gcc` — GCC_SLOT-dependent, written dynamically
///   in [`sandbox::Sandbox::setup_crossdev`].
const DEFAULT_FRAGMENTS: &[(&str, &str)] = &[
    (
        "env/plain.conf",
        include_str!("../config/portage/default/env/plain.conf"),
    ),
    (
        "package.env/rust",
        include_str!("../config/portage/default/package.env/rust"),
    ),
    (
        "package.use/busybox",
        include_str!("../config/portage/default/package.use/busybox"),
    ),
    (
        "package.use/clang",
        include_str!("../config/portage/default/package.use/clang"),
    ),
    (
        "package.use/git",
        include_str!("../config/portage/default/package.use/git"),
    ),
    (
        "package.use/rust",
        include_str!("../config/portage/default/package.use/rust"),
    ),
];

/// Write the default portage config fragments into `portage_dir`.
/// Creates parent directories as needed; overwrites existing files.
pub fn write_default_fragments(portage_dir: &Utf8Path) -> Result<()> {
    for (rel, content) in DEFAULT_FRAGMENTS {
        let path = portage_dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
    }
    Ok(())
}

/// Copy every regular file from `src` into `dst`, preserving directory layout.
/// Existing files at the destination are overwritten.
fn copy_tree(src: &Utf8Path, dst: &Utf8Path) -> Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.as_std_path().join(entry.file_name());
        if src_path.is_dir() {
            std::fs::create_dir_all(&dst_path)?;
            let src_utf8 = camino::Utf8PathBuf::try_from(src_path)
                .expect("portage overlay path is not UTF-8");
            let dst_utf8 = camino::Utf8PathBuf::try_from(dst_path)
                .expect("portage overlay path is not UTF-8");
            copy_tree(&src_utf8, &dst_utf8)?;
        } else if src_path.is_file() {
            if let Some(parent) = dst_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Write the portage config layers into `portage_dir`, applied in order so
/// later layers overwrite earlier ones on filename match:
///
///   1. embedded defaults (`config/portage/default/*`)
///   2. user-supplied overlay (`--portage-overlay <dir>`, optional)
pub fn write_portage_layers(
    portage_dir: &Utf8Path,
    user_overlay: Option<&Utf8Path>,
) -> Result<()> {
    write_default_fragments(portage_dir)?;
    if let Some(overlay) = user_overlay {
        if overlay.is_dir() {
            tracing::info!("Applying --portage-overlay {overlay}");
            copy_tree(overlay, portage_dir)?;
        }
    }
    Ok(())
}

/// Set or replace a variable in a make.conf file.
/// If the variable exists, replace its value; otherwise append.
pub fn set_make_conf_var(file: &Utf8Path, name: &str, value: &str) -> Result<()> {
    let content = std::fs::read_to_string(file).unwrap_or_default();
    let prefix = format!("{name}=");
    let new_line = format!("{name}=\"{value}\"");

    let mut found = false;
    let mut lines: Vec<String> = content
        .lines()
        .map(|line| {
            if line.starts_with(&prefix) {
                found = true;
                new_line.clone()
            } else {
                line.to_string()
            }
        })
        .collect();

    if !found {
        lines.push(new_line);
    }

    std::fs::write(file, lines.join("\n") + "\n")?;
    Ok(())
}

/// Portage operations that run *inside* a sandbox container.
pub struct Portage<'a> {
    runner: &'a SandboxRunner,
}

impl<'a> Portage<'a> {
    pub fn new(runner: &'a SandboxRunner) -> Self {
        Self { runner }
    }

    /// Initial sync of the portage tree.
    pub fn webrsync(&self) -> Result<()> {
        self.runner.run("emerge-webrsync")
    }

    /// `getuto` — fetch binary package signing keys (best-effort).
    pub fn getuto(&self) -> Result<()> {
        // Ignore failures: getuto may not be available or may fail on first run.
        let _ = self.runner.run("getuto");
        Ok(())
    }

    /// Emerge packages from binary packages only (`-G`).
    pub fn emerge_binary(&self, packages: &[&str]) -> Result<()> {
        let pkgs = packages.join(" ");
        self.runner.run(&format!("emerge -G {pkgs}"))
    }

    /// Emerge packages, using binary if available (`-b -k`).
    pub fn emerge(&self, packages: &[&str]) -> Result<()> {
        let pkgs = packages.join(" ");
        self.runner.run(&format!("emerge -b -k {pkgs}"))
    }

    /// Rebuild the world set.
    #[allow(dead_code)]
    pub fn emerge_world(&self) -> Result<()> {
        self.runner.run("emerge -b -k -e @world")
    }

    /// Cross-emerge packages into a target sysroot (mounted at `/target`).
    /// Uses `{chost}-emerge` which crossdev installs.
    pub fn cross_emerge(&self, chost: &str, packages: &[&str]) -> Result<()> {
        let pkgs = packages.join(" ");
        self.runner
            .run(&format!("ROOT=/target {chost}-emerge -b -k {pkgs}"))
    }

    /// Cross-emerge with `USE=build` for bootstrapping (baselayout, portage).
    pub fn cross_emerge_build(&self, chost: &str, packages: &[&str]) -> Result<()> {
        let pkgs = packages.join(" ");
        self.runner
            .run(&format!("USE=build ROOT=/target {chost}-emerge -b -k {pkgs}"))
    }

    /// Run `{chost}-emerge` without overriding ROOT, so packages install into
    /// the crossdev sysroot (`/usr/{chost}`) rather than `/target`.
    /// Used for updating the cross-toolchain itself (gcc, binutils-libs, @system).
    pub fn cross_emerge_sysroot(&self, chost: &str, packages: &[&str]) -> Result<()> {
        let pkgs = packages.join(" ");
        self.runner.run(&format!("{chost}-emerge -b -k {pkgs}"))
    }
}

/// Embedded default package lists. Kept as text files so adjusting
/// the host toolchain policy is a line-diff on `config/*.txt`.
pub const HOST_BIN_PACKAGES: &str = include_str!("../config/host-bin-packages.txt");
pub const HOST_PACKAGES: &str = include_str!("../config/host-packages.txt");
pub const CROSSDEV_EXTRA_PACKAGES: &str =
    include_str!("../config/crossdev-extra-packages.txt");

/// Parse a package-list file: one atom per line, `#` comments and blank lines ignored.
pub fn parse_package_list(content: &str) -> Vec<&str> {
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

/// Append per-board `make.conf` into `target_portage/make.conf`, wrapped in
/// markers so switching boards strips the previous board's block cleanly.
///
/// Boards should only set NEW variables here (USE, VIDEO_CARDS, ACCEPT_LICENSE).
/// Variables managed by [`MakeConf`] (FEATURES, MAKEOPTS, CHOST, CFLAGS, etc.)
/// are overwritten on stage1 re-runs; put those in `board.conf` workarounds instead.
pub fn apply_board_make_conf(
    target_portage: &Utf8Path,
    board_name: &str,
    boards_root: &Utf8Path,
) -> Result<()> {
    let make_conf = target_portage.join("make.conf");
    let existing = std::fs::read_to_string(&make_conf).unwrap_or_default();

    // Strip any previously injected board block.
    let mut stripped = String::new();
    let mut in_block = false;
    for line in existing.lines() {
        if line.starts_with("# [crossdev-stages: begin ") {
            in_block = true;
            continue;
        }
        if line.starts_with("# [crossdev-stages: end ") {
            in_block = false;
            continue;
        }
        if !in_block {
            stripped.push_str(line);
            stripped.push('\n');
        }
    }

    let board_file = boards_root.join(board_name).join("make.conf");
    if board_file.is_file() {
        let content = std::fs::read_to_string(&board_file)?;
        stripped.push_str(&format!(
            "# [crossdev-stages: begin boards/{board_name}/make.conf]\n"
        ));
        stripped.push_str(content.trim_end_matches('\n'));
        stripped.push('\n');
        stripped.push_str(&format!(
            "# [crossdev-stages: end boards/{board_name}/make.conf]\n"
        ));
    }

    std::fs::write(&make_conf, stripped)?;
    Ok(())
}

/// Install all host-side dependencies required for cross-compilation.
pub fn install_host_deps(runner: &SandboxRunner) -> Result<()> {
    let portage = Portage::new(runner);

    tracing::info!("Syncing portage tree…");
    portage.webrsync()?;
    let _ = portage.getuto();

    runner.run("chown -R portage:portage /etc/portage/gnupg")?;

    let bin_packages = parse_package_list(HOST_BIN_PACKAGES);
    tracing::info!("Installing binary packages…");
    portage.emerge_binary(&bin_packages)?;

    let packages = parse_package_list(HOST_PACKAGES);
    tracing::info!("Installing build dependencies…");
    portage.emerge(&packages)?;

    tracing::info!("Installing Rust ldconfig…");
    runner.run("cargo install --root /usr/local ldconfig")?;

    Ok(())
}

