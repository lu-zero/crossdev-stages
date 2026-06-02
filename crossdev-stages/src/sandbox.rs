use std::collections::HashMap;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use portage_atom::Version as PortageVersion;

use crate::board::BoardConfig;
use crate::container::{destroy_dir, unpack_tarball, SandboxRunner};
use crate::error::{Error, Result};
use crate::portage::{install_host_deps, MakeConf};
use crate::stage::gentoo_profile;
use crate::workspace::Workspace;

/// A Gentoo sandbox: an unpacked stage3 used as the host build environment.
pub struct Sandbox {
    pub dir: Utf8PathBuf,
    pub arch: String,
}

impl Sandbox {
    /// Open an existing sandbox directory, reading its `.arch` marker.
    pub fn open(dir: Utf8PathBuf) -> Result<Self> {
        let arch = std::fs::read_to_string(dir.join(".arch"))
            .map(|s| s.trim().to_string())
            .map_err(|_| Error::SandboxNotFound(dir.to_string()))?;
        Ok(Self { dir, arch })
    }

    /// Create a new sandbox by unpacking a stage3 source tarball (catalyst: `source_path`).
    /// Writes a `.arch` marker on success.
    pub fn create(ws: &Workspace, name: &str, arch: &str, source_stage: &Utf8Path) -> Result<Self> {
        let dir = ws.sandbox(name);
        if dir.is_dir() {
            tracing::info!("Sandbox {} already exists, skipping unpack.", name);
            return Self::open(dir);
        }
        tracing::info!("Unpacking stage3 into {}…", dir);
        unpack_tarball(source_stage, &dir, ws.base())?;
        std::fs::write(dir.join(".arch"), arch)?;
        tracing::info!("Sandbox {} created.", name);
        Ok(Self {
            dir,
            arch: arch.to_string(),
        })
    }

    /// Configure portage and install host build dependencies.
    /// Idempotent: skips if `.prepared` marker exists.
    pub fn prepare(&self, mirror: Option<&str>) -> Result<()> {
        if self.dir.join(".prepared").exists() {
            tracing::info!("Sandbox already prepared, skipping.");
            return Ok(());
        }
        tracing::info!("Configuring portage…");
        MakeConf {
            arch: &self.arch,
            chost: None,
            cflags: None,
            mirror,
            binhost: None,
        }
        .write(&self.dir.join("etc/portage"))?;

        tracing::info!("Installing host dependencies…");
        install_host_deps(&self.runner())?;

        std::fs::write(self.dir.join(".prepared"), "")?;
        tracing::info!("Sandbox prepared.");
        Ok(())
    }

    /// Return installed GCC versions grouped by slot, using `.gcc_versions` cache if present.
    /// Versions within each slot are sorted newest-first.
    pub fn get_installed_gcc_versions(&self) -> Result<HashMap<String, Vec<String>>> {
        let cache = self.dir.join(".gcc_versions");
        if let Ok(content) = fs::read_to_string(&cache) {
            if let Ok(parsed) = toml::from_str::<HashMap<String, Vec<String>>>(&content) {
                return Ok(parsed);
            }
        }
        self.refresh_gcc_versions()
    }

    /// Query installed GCC versions fresh via `qlist` and rewrite the `.gcc_versions` cache.
    fn refresh_gcc_versions(&self) -> Result<HashMap<String, Vec<String>>> {
        let output = self.runner().run_output("qlist -ICev sys-devel/gcc")?;
        let mut versions: HashMap<String, Vec<String>> = HashMap::new();
        for line in output.lines() {
            let line = line.trim();
            if let Some(ver_str) = line.split("/gcc-").last().filter(|s| !s.is_empty()) {
                if let Ok(ver) = PortageVersion::parse(ver_str) {
                    let slot = ver.numbers[0].to_string();
                    versions.entry(slot).or_default().push(ver_str.to_string());
                }
            }
        }
        for vs in versions.values_mut() {
            vs.sort_by(|a, b| b.cmp(a)); // newest first
        }
        let _ = fs::write(
            self.dir.join(".gcc_versions"),
            toml::to_string(&versions).unwrap_or_default(),
        );
        Ok(versions)
    }

    /// Set up the crossdev toolchain for `target_arch` inside this sandbox.
    /// Idempotent: skips if `.crossdev-<target_arch>` marker exists with a matching gcc version.
    ///
    /// GCC version resolution order: CLI `gcc_version` > `board.gcc_version` > highest installed slot.
    ///
    /// The spec is either a bare slot number ("15") or a version prefix ("15.2", "15.2.1_p20260214").
    /// Prefixes use portage's `=pkg-ver*` glob, so "15.2" matches any 15.2.x snapshot.
    pub fn setup_crossdev(
        &self,
        target_arch: &str,
        board: &BoardConfig,
        gcc_version: Option<&str>,
    ) -> Result<()> {
        let marker = self.dir.join(format!(".crossdev-{target_arch}"));
        let runner = self.runner();

        // Resolve spec from CLI > board > auto-detect.
        // A single number ("15") selects the slot; any longer version ("15.2", "15.2.1_p…")
        // is used as a portage version prefix via =sys-devel/gcc-<prefix>*.
        let requested = gcc_version.or(board.gcc_version.as_deref());
        let (gcc_slot, ver_prefix): (String, Option<String>) = match requested {
            None => {
                // Auto-detect: highest installed slot (avoids an extra gcc-config call).
                let installed = self.get_installed_gcc_versions()?;
                let slot = installed
                    .keys()
                    .filter_map(|k| k.parse::<u32>().ok())
                    .max()
                    .map(|n| n.to_string())
                    .ok_or_else(|| Error::CommandFailed {
                        code: 1,
                        reason: "No GCC installed in sandbox; run sandbox prepare first".into(),
                    })?;
                (slot, None)
            }
            Some(s) => {
                let ver = PortageVersion::parse(s).map_err(|_| Error::CommandFailed {
                    code: 1,
                    reason: format!("BOARD_GCC_VERSION {s:?} is not a valid portage version"),
                })?;
                let slot = ver.numbers[0].to_string();
                // A single bare number ("15") means slot-only; anything longer is a prefix.
                let is_slot_only =
                    ver.numbers.len() == 1 && ver.letter.is_none() && ver.suffixes.is_empty();
                if is_slot_only {
                    (slot, None)
                } else {
                    (slot, Some(s.to_string()))
                }
            }
        };

        // Idempotency: marker holds the exact gcc version used last time.
        // For a prefix spec, check that the installed version's numbers start with the prefix's.
        if marker.exists() {
            if let Ok(existing) = std::fs::read_to_string(&marker) {
                let existing = existing.trim();
                let matches = match &ver_prefix {
                    Some(prefix) => match (
                        PortageVersion::parse(existing),
                        PortageVersion::parse(prefix),
                    ) {
                        (Ok(ev), Ok(pv)) => ev.numbers.starts_with(&pv.numbers),
                        _ => existing.starts_with(prefix.as_str()),
                    },
                    None => {
                        PortageVersion::parse(existing)
                            .ok()
                            .and_then(|v| v.numbers.first().copied())
                            .map(|n| n.to_string())
                            .as_deref()
                            == Some(gcc_slot.as_str())
                    }
                };
                if matches {
                    tracing::info!(
                        "Crossdev for {target_arch} already set up with gcc-{existing}, skipping."
                    );
                    return Ok(());
                }
                let want = ver_prefix.as_deref().unwrap_or(&gcc_slot);
                tracing::info!(
                    "Crossdev for {target_arch}: re-setting up (was gcc-{existing}, want gcc-{want})…"
                );
                std::fs::remove_file(&marker)?;
            }
        }

        let chost = crate::stage::chost_for_arch(target_arch)?;
        let profile = gentoo_profile(target_arch)?;
        let cflags = board.effective_cflags();

        tracing::info!("Creating crossdev overlay…");
        runner.run(
            "eselect repository list -i | grep -q crossdev \
             || eselect repository create crossdev",
        )?;

        tracing::info!("Initialising crossdev for {chost}…");
        runner.run(&format!("crossdev {chost} --init-target"))?;

        // Accept testing/prerelease keywords for this gcc slot and rust-std.
        runner.run(&format!(
            "echo 'cross-{chost}/rust-std **' \
             > /etc/portage/package.accept_keywords/rust-std"
        ))?;
        let gcc_keyword_line = format!("sys-devel/gcc:{gcc_slot} **");
        runner.run(&format!(
            "echo '{gcc_keyword_line}' > /etc/portage/package.accept_keywords/gcc"
        ))?;

        // Emerge: version prefix glob (quoted to prevent shell expansion) or best-in-slot.
        if let Some(ref prefix) = ver_prefix {
            tracing::info!("Emerging =sys-devel/gcc-{prefix}* (host)…");
            runner.run(&format!("emerge -b -k '=sys-devel/gcc-{prefix}*'"))?;
        } else {
            tracing::info!("Emerging sys-devel/gcc:{gcc_slot} (host)…");
            runner.run(&format!("emerge -b -k sys-devel/gcc:{gcc_slot}"))?;
        }

        // Refresh metadata after emerge — never use the pre-emerge cache here.
        let installed = self.refresh_gcc_versions()?;
        let slot_versions = installed.get(&gcc_slot).cloned().unwrap_or_default();

        let gcc_ver = if let Some(ref prefix) = ver_prefix {
            slot_versions
                .into_iter()
                .find(|v| v.starts_with(prefix.as_str()))
                .ok_or_else(|| Error::CommandFailed {
                    code: 1,
                    reason: format!("No gcc-{prefix}* found in sandbox after emerge"),
                })?
        } else {
            // Slot-only: newest installed version (refresh gives newest-first).
            slot_versions
                .into_iter()
                .next()
                .ok_or_else(|| Error::CommandFailed {
                    code: 1,
                    reason: format!("No gcc:{gcc_slot} found in sandbox after emerge"),
                })?
        };

        tracing::info!("Using gcc-{gcc_ver} for crossdev.");

        // gcc-config profile names are "{chost}-{slot}" (e.g. "aarch64-unknown-linux-gnu-15"),
        // not "{chost}-{full-version}". Select by slot directly.
        let host_chost = runner
            .run_output("portageq envvar CHOST")?
            .trim()
            .to_string();
        runner.run(&format!("gcc-config {host_chost}-{gcc_slot}"))?;
        runner.run("env-update && source /etc/profile")?;

        // Configure the crossdev prefix portage settings (written on the host fs).
        let crossdev_root = self.dir.join(format!("usr/{chost}"));
        let crossdev_portage = crossdev_root.join("etc/portage");
        runner.run(&format!(
            "export PORTAGE_CONFIGROOT=/usr/{chost}; eselect profile set {profile}"
        ))?;
        self.write_crossdev_portage(
            &crossdev_portage,
            target_arch,
            &chost,
            &cflags,
            board,
            &gcc_keyword_line,
        )?;

        // Fix the split-usr layout created by crossdev.
        runner.run(&format!("mkdir -p /usr/{chost}/bin"))?;
        runner.run(&format!("merge-usr --root /usr/{chost}"))?;

        tracing::info!("Running crossdev (this takes a while)…");
        runner.run(&format!(
            "crossdev {chost} \
             --gcc {gcc_ver} \
             --ex-pkg sys-devel/clang-crossdev-wrappers \
             --ex-pkg sys-devel/rust-std"
        ))?;

        // Switch cross compiler to the installed slot.
        runner.run(&format!(
            "gcc-config {chost}-{gcc_slot} && source /etc/profile"
        ))?;

        // Write marker with the exact version used (enables idempotency on next run).
        std::fs::write(&marker, &gcc_ver)?;
        tracing::info!("Crossdev for {chost} complete (gcc-{gcc_ver}).");
        Ok(())
    }

    /// Return a `SandboxRunner` for running commands inside this sandbox.
    /// Logs are bind-mounted from `~/.cache/crossdev-stages/logs/<name>/`
    /// so they are accessible outside the sandbox at a known flat path.
    pub fn runner(&self) -> SandboxRunner {
        let name = self.dir.file_name().unwrap_or_default();
        let log_dir = self
            .dir
            .parent() // sandboxes/
            .and_then(|p| p.parent()) // <workspace>/
            .map(|ws| ws.join("logs").join(name))
            .unwrap_or_else(|| self.dir.join("var/log"));
        SandboxRunner::new(&self.dir, log_dir)
    }

    #[allow(dead_code)]
    pub fn is_prepared(&self) -> bool {
        self.dir.join(".prepared").exists()
    }

    #[allow(dead_code)]
    pub fn has_crossdev(&self, target_arch: &str) -> bool {
        self.dir.join(format!(".crossdev-{target_arch}")).exists()
    }

    // ── Private helpers ──────────────────────────────────────────────────────

    /// Write portage config files for the crossdev prefix directly on the host fs.
    fn write_crossdev_portage(
        &self,
        portage_dir: &Utf8Path,
        arch: &str,
        chost: &str,
        cflags: &str,
        board: &BoardConfig,
        gcc_keyword_line: &str,
    ) -> Result<()> {
        // make.conf for the crossdev prefix
        MakeConf {
            arch,
            chost: Some(chost),
            cflags: Some(cflags),
            mirror: None,
            binhost: None,
        }
        .write(portage_dir)?;

        for sub in [
            "env",
            "package.env",
            "package.use",
            "package.accept_keywords",
        ] {
            std::fs::create_dir_all(portage_dir.join(sub))?;
        }

        // env/plain.conf: strip arch-specific flags (used for rust, etc.)
        std::fs::write(
            portage_dir.join("env/plain.conf"),
            "CFLAGS=\"-O3 -pipe\"\nCXXFLAGS=\"-O3 -pipe\"\n",
        )?;

        // package.env
        std::fs::write(
            portage_dir.join("package.env/rust"),
            "dev-lang/rust plain.conf\n",
        )?;

        // package.use
        std::fs::write(
            portage_dir.join("package.use/busybox"),
            ">=virtual/libcrypt-2-r1 static-libs\n\
             >=sys-libs/libxcrypt-4.4.36-r3 static-libs\n\
             >=sys-apps/busybox-1.36.1-r3 -pam static\n",
        )?;
        std::fs::write(
            portage_dir.join("package.use/clang"),
            "llvm-core/clang -extra\n",
        )?;
        std::fs::write(
            portage_dir.join("package.use/rust"),
            "dev-lang/rust rustfmt -system-llvm\n",
        )?;
        std::fs::write(portage_dir.join("package.use/git"), "dev-vcs/git -iconv\n")?;

        // package.accept_keywords
        std::fs::write(
            portage_dir.join("package.accept_keywords/gcc"),
            format!("{gcc_keyword_line}\n"),
        )?;

        // Per-package CFLAGS workarounds from board.conf
        for (pkg, flags) in board
            .workaround_pkgs
            .iter()
            .zip(board.workaround_cflags.iter())
        {
            let safe_name = pkg.replace('/', "_");
            std::fs::write(
                portage_dir.join(format!("env/{safe_name}.conf")),
                format!("CFLAGS=\"{flags}\"\nCXXFLAGS=\"{flags}\"\n"),
            )?;
            std::fs::write(
                portage_dir.join(format!("package.env/{safe_name}")),
                format!("{pkg} {safe_name}.conf\n"),
            )?;
        }

        Ok(())
    }
}

/// Remove a sandbox directory (via hakoniwa to handle root-owned files from stage3).
pub fn destroy(ws: &Workspace, name: &str) -> Result<()> {
    let dir = ws.sandbox(name);
    if !dir.is_dir() {
        return Err(crate::error::Error::SandboxNotFound(name.into()));
    }
    println!("Removing sandbox: {name}");
    destroy_dir(&dir, ws.base())?;
    println!("Sandbox '{name}' removed.");
    Ok(())
}

/// List all sandbox directories with their state.
pub fn list(ws: &Workspace) -> Result<Vec<SandboxInfo>> {
    let dirs = ws.list_sandboxes()?;
    Ok(dirs
        .into_iter()
        .map(|dir| {
            let arch = crate::workspace::read_arch(&dir).unwrap_or_else(|| "unknown".into());
            let prepared = dir.join(".prepared").exists();
            let name = dir.file_name().unwrap_or("").to_string();
            SandboxInfo {
                name,
                arch,
                prepared,
            }
        })
        .collect())
}

pub struct SandboxInfo {
    pub name: String,
    pub arch: String,
    pub prepared: bool,
}
