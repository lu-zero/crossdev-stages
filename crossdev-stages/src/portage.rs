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
}

impl<'a> MakeConf<'a> {
    /// Write `make.conf` into `portage_dir` (i.e. `/etc/portage` of a sandbox or target stage).
    /// Updates variables in-place; preserves any existing content not managed here.
    pub fn write(&self, portage_dir: &Utf8Path) -> Result<()> {
        std::fs::create_dir_all(portage_dir)?;
        std::fs::create_dir_all(portage_dir.join("package.accept_keywords"))?;

        let make_conf = portage_dir.join("make.conf");
        if !make_conf.exists() {
            std::fs::write(&make_conf, "")?;
        }
        for (name, value) in self.vars()? {
            set_make_conf_var(&make_conf, name, &value)?;
        }
        Ok(())
    }

    /// The variables [`write`](Self::write) sets, in file order.
    pub fn vars(&self) -> Result<Vec<(&'static str, String)>> {
        let (jobs, load) = parallelism();
        let garch = gentoo_arch(self.arch)?;
        let cflags = self.cflags.unwrap_or_else(|| default_cflags(self.arch));
        let features = if self.binhost.is_some() {
            "parallel-install -merge-wait getbinpkg"
        } else {
            "parallel-install -merge-wait"
        };

        let mut vars = vec![
            ("MAKEOPTS", format!("-j{jobs} --load-average {load}")),
            (
                "EMERGE_DEFAULT_OPTS",
                format!("--jobs={jobs} --load-average {load}"),
            ),
            ("FEATURES", features.to_string()),
            ("ACCEPT_KEYWORDS", format!("~{garch}")),
            ("PORT_LOGDIR", format!("/var/log/portage/{garch}")),
        ];

        // LLVM_TARGETS: host gets the union of every supported arch (so the
        // bundled LLVM inside dev-lang/rust can bootstrap any cross-std);
        // cross-sysroots get only their own arch target.
        let llvm_targets = match self.chost {
            Some(_) => llvm_target(self.arch).map(str::to_string),
            None => Some(all_llvm_targets()),
        };
        if let Some(targets) = llvm_targets.filter(|s| !s.is_empty()) {
            vars.push(("LLVM_TARGETS", targets));
        }

        if let Some(chost) = self.chost {
            vars.push(("CHOST", chost.to_string()));
            vars.push(("CFLAGS", cflags.to_string()));
            vars.push(("CXXFLAGS", cflags.to_string()));
        }
        if let Some(mirror) = self.mirror {
            vars.push(("GENTOO_MIRRORS", mirror.to_string()));
        }
        if let Some(binhost) = self.binhost {
            vars.push(("PORTAGE_BINHOST", binhost.to_string()));
        }
        Ok(vars)
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

/// What installs packages in a sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Portage's `emerge`, with crossdev and its `<chost>-emerge` wrappers.
    Portage,
    /// `em`, for both the host and the cross toolchain.
    Em,
}

/// A cross install command: `<chost>-emerge` with Portage, `em --target <chost>` with em.
///
/// `env` is a list of `VAR=value` words; `root` and `config_root` become
/// `ROOT`/`PORTAGE_CONFIGROOT` for Portage and `--root`/`--config-root` for em.
fn cross_command(
    backend: Backend,
    chost: &str,
    env: &str,
    root: Option<&str>,
    config_root: Option<&str>,
    args: &str,
) -> String {
    let mut words = vec![env.to_string()];
    match backend {
        Backend::Portage => {
            words.extend(config_root.map(|c| format!("PORTAGE_CONFIGROOT={c}")));
            words.extend(root.map(|r| format!("ROOT={r}")));
            words.push(format!("{chost}-emerge"));
        }
        Backend::Em => {
            words.push(format!("em --target {chost}"));
            words.extend(root.map(|r| format!("--root {r}")));
            words.extend(config_root.map(|c| format!("--config-root {c}")));
        }
    }
    words.push(args.to_string());
    words.retain(|w| !w.is_empty());
    words.join(" ")
}

/// Portage operations that run *inside* a sandbox container.
pub struct Portage<'a> {
    runner: &'a SandboxRunner,
    backend: Backend,
}

impl<'a> Portage<'a> {
    pub fn new(runner: &'a SandboxRunner, backend: Backend) -> Self {
        Self { runner, backend }
    }

    fn cross(
        &self,
        chost: &str,
        env: &str,
        root: Option<&str>,
        config_root: Option<&str>,
        args: &str,
    ) -> Result<()> {
        let cmd = cross_command(self.backend, chost, env, root, config_root, args);
        self.runner.run(&cmd)
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
        let emerge = match self.backend {
            Backend::Portage => "emerge",
            Backend::Em => "em",
        };
        self.runner.run(&format!("{emerge} -b -k {pkgs}"))
    }

    /// Rebuild the world set.
    #[allow(dead_code)]
    pub fn emerge_world(&self) -> Result<()> {
        self.runner.run("emerge -b -k -e @world")
    }

    /// Cross-emerge packages into the target stage (mounted at `/target`).
    pub fn cross_emerge(&self, chost: &str, packages: &[&str]) -> Result<()> {
        let pkgs = packages.join(" ");
        self.cross(chost, "", Some("/target"), None, &format!("-b -k {pkgs}"))
    }

    /// Upgrade the target `@world` and `packages` to the best versions in the
    /// synced tree, including deep runtime dependencies and changed USE flags.
    ///
    /// `SYSROOT` is the target, so a dependency is installed once, into the
    /// image, rather than again into the cross prefix. Live gcc is masked
    /// first; the cross compiler stays the host's `cross-*/gcc`.
    pub fn cross_emerge_update(&self, chost: &str, packages: &[&str]) -> Result<()> {
        reject_live_target_gcc(self.runner.sandbox_dir(), chost)?;
        let extra = packages.join(" ");
        let atoms = if extra.is_empty() {
            "@world".to_string()
        } else {
            format!("@world {extra}")
        };
        self.cross(
            chost,
            "SYSROOT=/target",
            Some("/target"),
            Some(&format!("/usr/{chost}")),
            &format!("-b -k -uDN --with-bdeps=n {atoms}"),
        )
    }

    /// Cross-emerge with `USE=build` for bootstrapping (baselayout, portage).
    pub fn cross_emerge_build(&self, chost: &str, packages: &[&str]) -> Result<()> {
        let pkgs = packages.join(" ");
        let args = format!("-b -k {pkgs}");
        self.cross(chost, "USE=build", Some("/target"), None, &args)
    }

    /// Rebuild the target stage's `@world` with `KERNEL_DIR` pointing at the
    /// in-prefix kernel headers, which some `@world` packages read.
    pub fn cross_emerge_world(&self, chost: &str) -> Result<()> {
        let env = "KERNEL_DIR=/usr/src/linux";
        self.cross(chost, env, Some("/target"), None, "-b -k -e @world")
    }

    /// `packages.build` lines from the target profile's gentoo repo, joined
    /// for a single cross-emerge. Fails if the file is missing or empty.
    pub fn packages_build(&self) -> Result<String> {
        let packages = self.runner.run_output(
            "grep -v '^#' /var/db/repos/gentoo/profiles/default/linux/packages.build \
             | grep -v '^[[:space:]]*$' | tr '\\n' ' '",
        )?;
        if packages.is_empty() {
            return Err(crate::error::Error::CommandFailed {
                code: 1,
                reason: "packages.build is empty or missing".into(),
            });
        }
        Ok(packages)
    }

    /// Cross-emerge without overriding ROOT, so packages install into
    /// the crossdev prefix (`/usr/{chost}`) rather than `/target`.
    /// Used for updating the cross-toolchain itself (gcc, binutils-libs, @system).
    pub fn cross_emerge_crossdev(&self, chost: &str, packages: &[&str]) -> Result<()> {
        let pkgs = packages.join(" ");
        self.cross(chost, "", None, None, &format!("-b -k {pkgs}"))
    }

    /// Emerge the host `sys-devel/gcc` cross-compiler seed into the sandbox.
    ///
    /// `ver_prefix` selects a version glob (`=sys-devel/gcc-15.2*`, quoted so the
    /// shell cannot expand it); `None` takes the best available in the slot.
    pub fn emerge_host_gcc(&self, slot: &str, ver_prefix: Option<&str>) -> Result<()> {
        match ver_prefix {
            Some(prefix) => {
                tracing::info!("Emerging =sys-devel/gcc-{prefix}* (host)…");
                self.runner
                    .run(&format!("emerge -b -k '=sys-devel/gcc-{prefix}*'"))
            }
            None => {
                tracing::info!("Emerging sys-devel/gcc:{slot} (host)…");
                self.runner
                    .run(&format!("emerge -b -k sys-devel/gcc:{slot}"))
            }
        }
    }

    // ── Crossdev prefix setup ────────────────────────────────────────────────

    /// Create the `crossdev` overlay repository if it isn't registered yet.
    ///
    /// Match the exact `[crossdev]` section with a `location`, not any name
    /// containing "crossdev": `crossdev --init-target` also registers a
    /// `crossdev.<chost>` alias repo, and a substring grep would mistake that
    /// for the output overlay and skip creating it.
    pub fn ensure_crossdev_repo(&self) -> Result<()> {
        self.runner.run(
            "if grep -qsE '^\\[crossdev\\]$' /etc/portage/repos.conf/*.conf 2>/dev/null; then \
                 true; \
             else \
                 eselect repository create crossdev; \
             fi",
        )
    }

    /// `crossdev <chost> --init-target`: overlay alias and sysroot skeleton,
    /// no toolchain built yet.
    pub fn crossdev_init(&self, chost: &str) -> Result<()> {
        self.runner.run(&format!("crossdev {chost} --init-target"))
    }

    /// `crossdev <chost> --gcc <ver> --ex-pkg …`: the toolchain build itself.
    pub fn crossdev_setup(&self, chost: &str, gcc_ver: &str, ex_pkgs: &[&str]) -> Result<()> {
        let mut cmd = format!("crossdev {chost} --gcc {gcc_ver}");
        for pkg in ex_pkgs {
            cmd.push_str(" --ex-pkg ");
            cmd.push_str(pkg);
        }
        self.runner.run(&cmd)
    }

    /// Point `PORTAGE_CONFIGROOT` at the crossdev prefix and pick its profile.
    pub fn crossdev_set_profile(&self, chost: &str, profile: &str) -> Result<()> {
        self.runner.run(&format!(
            "export PORTAGE_CONFIGROOT=/usr/{chost}; eselect profile set {profile}"
        ))
    }

    /// Repair the split-usr layout crossdev leaves behind in the prefix.
    pub fn crossdev_merge_usr(&self, chost: &str) -> Result<()> {
        self.runner.run(&format!("mkdir -p /usr/{chost}/bin"))?;
        self.runner.run(&format!("merge-usr --root /usr/{chost}"))
    }

    /// Write one `package.accept_keywords` line into the sandbox's `/etc/portage`.
    pub fn accept_keywords(&self, name: &str, line: &str) -> Result<()> {
        self.runner.run(&format!(
            "echo '{line}' > /etc/portage/package.accept_keywords/{name}"
        ))
    }

    // ── Toolchain activation ────────────────────────────────────────────────

    /// Activate the `{chost}-{slot}` gcc profile.
    pub fn activate_gcc(&self, chost: &str, slot: &str) -> Result<()> {
        self.runner.run(&format!("gcc-config {chost}-{slot}"))
    }

    /// Re-read `/etc/profile` so an activated compiler reaches `PATH`.
    pub fn activate_gcc_and_profile(&self, chost: &str, slot: &str) -> Result<()> {
        self.runner
            .run(&format!("gcc-config {chost}-{slot} && source /etc/profile"))
    }

    /// Regenerate `/etc/profile.env` from `/etc/env.d` and re-read it.
    pub fn refresh_profile_env(&self) -> Result<()> {
        self.runner.run("env-update && source /etc/profile")
    }

    // ── Queries ─────────────────────────────────────────────────────────────

    /// The sandbox host's own `CHOST`, as `portageq` reports it.
    pub fn host_chost(&self) -> Result<String> {
        Ok(self
            .runner
            .run_output("portageq envvar CHOST")?
            .trim()
            .to_string())
    }

    /// Installed versions of `cpn`, one per line, as `qlist -ICev` reports them.
    pub fn installed_versions(&self, cpn: &str) -> Result<String> {
        self.runner.run_output(&format!("qlist -ICev {cpn}"))
    }
}

/// Keep live `sys-devel/gcc` out of the image. The host keyword accept is
/// untouched, so the cross compiler itself can stay a live ebuild.
pub fn reject_live_target_gcc(sandbox: &Utf8Path, chost: &str) -> Result<()> {
    let portage_dir = sandbox.join(format!("usr/{chost}/etc/portage"));
    let keywords = portage_dir.join("package.accept_keywords/gcc");
    if keywords.is_file() {
        std::fs::remove_file(keywords)?;
    }
    let gcc_dir = sandbox.join("var/db/repos/gentoo/sys-devel/gcc");
    let names = match std::fs::read_dir(&gcc_dir) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    let mask_dir = portage_dir.join("package.mask");
    std::fs::create_dir_all(&mask_dir)?;
    std::fs::write(mask_dir.join("gcc-live"), gcc_live_atoms(&names))?;
    Ok(())
}

/// `package.mask` lines for live gcc ebuilds. Release ebuilds are omitted.
fn gcc_live_atoms(ebuild_names: &[String]) -> String {
    let mut lines: Vec<String> = ebuild_names
        .iter()
        .filter_map(|name| {
            let ver = name.strip_prefix("gcc-")?.strip_suffix(".ebuild")?;
            ver.contains(".9999")
                .then(|| format!("=sys-devel/gcc-{ver}\n"))
        })
        .collect();
    lines.sort();
    lines.concat()
}

/// Sync the portage tree inside a sandbox (emerge-webrsync, signing keys).
pub fn sync_portage_tree(runner: &SandboxRunner) -> Result<()> {
    let portage = Portage::new(runner, Backend::Portage);

    tracing::info!("Syncing portage tree…");
    portage.webrsync()?;
    let _ = portage.getuto();

    runner.run("chown -R portage:portage /etc/portage/gnupg")?;
    Ok(())
}

/// Install all host-side dependencies required for cross-compilation, always with emerge.
pub fn install_host_deps(runner: &SandboxRunner) -> Result<()> {
    sync_portage_tree(runner)?;
    let portage = Portage::new(runner, Backend::Portage);

    let bin_packages = ["app-arch/zstd", "app-arch/bzip2", "app-arch/xz-utils"];
    tracing::info!("Installing binary packages…");
    portage.emerge_binary(&bin_packages)?;

    let packages = [
        "sys-devel/crossdev",
        "sys-devel/bc",
        "sys-apps/merge-usr",
        "dev-vcs/git",
        "dev-embedded/u-boot-tools",
        "sys-apps/dtc",
        "sys-kernel/dracut",
        "sys-apps/busybox",
        "sys-fs/genimage",
        "sys-fs/dosfstools",
        "sys-fs/mtools",
        "app-eselect/eselect-repository",
        "dev-lang/rust",
        "dev-python/pyelftools",
    ];
    tracing::info!("Installing build dependencies…");
    portage.emerge(&packages)?;

    tracing::info!("Installing Rust ldconfig…");
    runner.run("cargo install --root /usr/local ldconfig")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{cross_command, gcc_live_atoms, Backend};

    #[test]
    fn cross_command_maps_roots_per_backend() {
        let chost = "riscv64-unknown-linux-gnu";
        let configroot = Some("/usr/riscv64-unknown-linux-gnu");
        let update = |b| {
            cross_command(
                b,
                chost,
                "SYSROOT=/target",
                Some("/target"),
                configroot,
                "-uDN @world",
            )
        };
        assert_eq!(
            update(Backend::Portage),
            "SYSROOT=/target PORTAGE_CONFIGROOT=/usr/riscv64-unknown-linux-gnu ROOT=/target \
             riscv64-unknown-linux-gnu-emerge -uDN @world"
        );
        assert_eq!(
            update(Backend::Em),
            "SYSROOT=/target em --target riscv64-unknown-linux-gnu --root /target \
             --config-root /usr/riscv64-unknown-linux-gnu -uDN @world"
        );
        assert_eq!(
            cross_command(Backend::Em, chost, "", None, None, "-b -k sys-boot/grub"),
            "em --target riscv64-unknown-linux-gnu -b -k sys-boot/grub"
        );
    }

    #[test]
    fn live_gcc_mask_skips_release_ebuilds() {
        let body = gcc_live_atoms(&[
            "gcc-16.2.1_p20260926.ebuild".into(),
            "gcc-16.3.9999.ebuild".into(),
            "gcc-16.3.9999-r1.ebuild".into(),
            "Manifest".into(),
        ]);
        assert_eq!(
            body,
            "=sys-devel/gcc-16.3.9999\n=sys-devel/gcc-16.3.9999-r1\n"
        );
    }
}
