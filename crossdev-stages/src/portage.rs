use camino::Utf8Path;

use crate::container::SandboxRunner;
use crate::error::Result;
use crate::stage::{all_llvm_targets, default_cflags, gentoo_arch, llvm_target};

/// Parameters for a Portage `make.conf` file.
pub struct MakeConf<'a> {
    pub arch: &'a str,
    pub chost: Option<&'a str>,
    pub cflags: Option<&'a str>,
    pub mirror: Option<&'a str>,
    pub binhost: Option<&'a str>,
    /// In-container path of a writable bind-mount where binpkgs should
    /// land.  When set, `FEATURES` gains `buildpkg` and `PKGDIR` is
    /// pointed at it.  `None` keeps portage defaults.
    pub pkgdir: Option<&'a str>,
}

impl<'a> MakeConf<'a> {
    /// Write `make.conf` into `portage_dir` (i.e. `/etc/portage` of a sandbox or target stage).
    /// Updates variables in-place; preserves any existing content not managed here.
    pub fn write(&self, portage_dir: &Utf8Path) -> Result<()> {
        std::fs::create_dir_all(portage_dir)?;
        std::fs::create_dir_all(portage_dir.join("package.accept_keywords"))?;
        std::fs::create_dir_all(portage_dir.join("package.mask"))?;

        let make_conf = portage_dir.join("make.conf");
        if !make_conf.exists() {
            std::fs::write(&make_conf, "")?;
        }

        // Clean up the llvm:22 mask left in existing sandboxes.
        let _ = std::fs::remove_file(portage_dir.join("package.mask/llvm-unused-slot"));

        let (jobs, load) = parallelism();
        let garch = gentoo_arch(self.arch)?;
        let cflags = self.cflags.unwrap_or_else(|| default_cflags(self.arch));

        set_make_conf_var(
            &make_conf,
            "MAKEOPTS",
            &format!("-j{jobs} --load-average {load}"),
        )?;
        set_make_conf_var(
            &make_conf,
            "EMERGE_DEFAULT_OPTS",
            &format!("--jobs={jobs} --load-average {load}"),
        )?;
        let features = if self.pkgdir.is_some() {
            "parallel-install -merge-wait buildpkg"
        } else {
            "parallel-install -merge-wait"
        };
        set_make_conf_var(&make_conf, "FEATURES", features)?;
        set_make_conf_var(&make_conf, "ACCEPT_KEYWORDS", &format!("~{garch}"))?;
        set_make_conf_var(
            &make_conf,
            "PORT_LOGDIR",
            &format!("/var/log/portage/{garch}"),
        )?;
        // PKGDIR only makes sense inside the sandbox (it names a bind-mount
        // path).  Drop a stale line when unset so a target make.conf that
        // once carried it (and would ship into images) is healed on the
        // next prepare.
        match self.pkgdir {
            Some(pkgdir) => set_make_conf_var(&make_conf, "PKGDIR", pkgdir)?,
            None => remove_make_conf_var(&make_conf, "PKGDIR")?,
        }

        // LLVM_TARGETS: host gets the union of every supported arch (so the
        // bundled LLVM inside dev-lang/rust can bootstrap any cross-std);
        // cross-sysroots get only their own arch target.
        let llvm_targets = match self.chost {
            Some(_) => llvm_target(self.arch).map(str::to_string),
            None => Some(all_llvm_targets()),
        };
        if let Some(targets) = llvm_targets.filter(|s| !s.is_empty()) {
            set_make_conf_var(&make_conf, "LLVM_TARGETS", &targets)?;
        }

        if let Some(chost) = self.chost {
            set_make_conf_var(&make_conf, "CHOST", chost)?;
            set_make_conf_var(&make_conf, "CFLAGS", cflags)?;
            set_make_conf_var(&make_conf, "CXXFLAGS", cflags)?;
        }

        if let Some(mirror) = self.mirror {
            set_make_conf_var(&make_conf, "GENTOO_MIRRORS", mirror)?;
        }

        if let Some(binhost) = self.binhost {
            set_make_conf_var(&make_conf, "PORTAGE_BINHOST", binhost)?;
            let features = if self.pkgdir.is_some() {
                "parallel-install -merge-wait buildpkg getbinpkg"
            } else {
                "parallel-install -merge-wait getbinpkg"
            };
            set_make_conf_var(&make_conf, "FEATURES", features)?;
        }

        Ok(())
    }
}

fn parallelism() -> (usize, usize) {
    let n = num_cpus::get();
    let jobs = n / 2 + 1;
    let load = n;
    (jobs, load)
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

/// Remove a variable from a make.conf file if present.
fn remove_make_conf_var(file: &Utf8Path, name: &str) -> Result<()> {
    let content = std::fs::read_to_string(file).unwrap_or_default();
    let prefix = format!("{name}=");
    let lines: Vec<&str> = content
        .lines()
        .filter(|line| !line.starts_with(&prefix))
        .collect();
    std::fs::write(file, lines.join("\n") + "\n")?;
    Ok(())
}

/// Print the tail of every recent portage log that recorded a failure.
/// Falls back to naming the newest logs when nothing matched, so the failure
/// is never reported with no way to look further.
const SHOW_BUILD_FAILURES: &str = r#"
logs=$(ls -t /var/log/portage/*/*.log 2>/dev/null | head -n 40)
[ -n "$logs" ] || exit 0
hit=$(grep -l " \* ERROR: " $logs 2>/dev/null | head -n 3)
if [ -n "$hit" ]; then
    for f in $hit; do
        printf "\n--- %s ---\n" "$f"
        tail -n 80 "$f"
    done
else
    printf "\nNo build log recorded an ERROR.  Most recent logs:\n"
    printf "%s\n" "$logs" | head -n 5
fi
"#;

/// Portage operations that run *inside* a sandbox container.
pub struct Portage<'a> {
    runner: &'a SandboxRunner,
}

impl<'a> Portage<'a> {
    pub fn new(runner: &'a SandboxRunner) -> Self {
        Self { runner }
    }

    /// Run an emerge and, if it fails, print the build log that recorded why.
    ///
    /// Portage switches to background mode whenever EMERGE_DEFAULT_OPTS carries
    /// `--jobs` (Scheduler._background_mode), so a failing build writes nothing
    /// to stdout; the compiler output only ever reaches PORT_LOGDIR.  That
    /// directory is owned by the sandbox uid and mode 0770, so it cannot be
    /// read from the host either -- the tail has to be taken from inside.
    fn run_emerge(&self, cmd: &str) -> Result<()> {
        let result = self.runner.run(cmd);
        if result.is_err() {
            let _ = self.runner.run(SHOW_BUILD_FAILURES);
        }
        result
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

    /// Emerge packages, using binary if available (`-b -k`).
    /// `--changed-use` so a board's `sandbox-packages.use` also applies to
    /// packages that are already installed with different flags.
    pub fn emerge(&self, packages: &[&str]) -> Result<()> {
        let pkgs = packages.join(" ");
        self.run_emerge(&format!("emerge -b -k --changed-use {pkgs}"))
    }

    /// Rebuild the world set.
    #[allow(dead_code)]
    pub fn emerge_world(&self) -> Result<()> {
        self.run_emerge("emerge -b -k -e @world")
    }

    /// Cross-emerge packages into the target stage (mounted at `/target`).
    /// Uses `{chost}-emerge` which crossdev installs.
    pub fn cross_emerge(&self, chost: &str, packages: &[&str]) -> Result<()> {
        let pkgs = packages.join(" ");
        self.run_emerge(&format!("ROOT=/target {chost}-emerge -b -k {pkgs}"))
    }

    /// Cross-emerge with `USE=build` for bootstrapping (baselayout, portage).
    pub fn cross_emerge_build(&self, chost: &str, packages: &[&str]) -> Result<()> {
        let pkgs = packages.join(" ");
        self.run_emerge(&format!(
            "USE=build ROOT=/target {chost}-emerge -b -k {pkgs}"
        ))
    }

    /// Run `{chost}-emerge` without overriding ROOT, so packages install into
    /// the crossdev prefix (`/usr/{chost}`) rather than `/target`.
    /// Used for updating the cross-toolchain itself (gcc, binutils-libs, @system).
    pub fn cross_emerge_crossdev(&self, chost: &str, packages: &[&str]) -> Result<()> {
        let pkgs = packages.join(" ");
        self.run_emerge(&format!("{chost}-emerge -b -k {pkgs}"))
    }
}

/// Sync the portage tree inside a sandbox (emerge-webrsync, signing keys).
pub fn sync_portage_tree(runner: &SandboxRunner) -> Result<()> {
    let portage = Portage::new(runner);

    tracing::info!("Syncing portage tree…");
    portage.webrsync()?;
    let _ = portage.getuto();

    runner.run("chown -R portage:portage /etc/portage/gnupg")?;
    Ok(())
}

/// Install all host-side dependencies required for cross-compilation.
///
/// Reads the package list from `<defaults_root>/sandbox-packages.txt`. Per-board
/// extras (boards/<board>/sandbox-packages.txt) are emerged separately during
/// image build via `image::default_deps`. Keyword overrides (`atom [keywords]`
/// lines) are written to `<portage_dir>/package.accept_keywords/`.
pub fn install_host_deps(
    runner: &SandboxRunner,
    defaults_root: &Utf8Path,
    portage_dir: &Utf8Path,
) -> Result<()> {
    sync_portage_tree(runner)?;
    let portage = Portage::new(runner);

    let path = defaults_root.join("sandbox-packages.txt");
    let packages = crate::package_list::read_required(&path)?;
    crate::package_list::write_accept_keywords(&packages, portage_dir)?;

    tracing::info!(
        "Installing host build dependencies ({} packages)…",
        packages.len()
    );
    portage.emerge(&crate::package_list::atoms(&packages))?;

    tracing::info!("Installing Rust ldconfig…");
    runner.run("cargo install --root /usr/local ldconfig")?;

    Ok(())
}
