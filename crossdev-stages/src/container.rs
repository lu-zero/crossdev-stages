use camino::{Utf8Path, Utf8PathBuf};
use hakoniwa::{Container, Namespace, Runctl};

use crate::error::{check_status, Result};

/// One overlayfs mount to perform inside the sandbox before the user
/// command runs.  Phase 3 uses this to overlay a content-addressed
/// crossdev prefix (`store/<chost>/<cflags-hash>/`) at `/usr/<chost>/`.
///
/// All three host directories are bind-mounted into the container at
/// hidden paths and then `mount -t overlay` is invoked from inside the
/// userns (where uid 0 has CAP_SYS_ADMIN).
#[derive(Clone, Debug)]
pub struct OverlaySpec {
    pub lower: Utf8PathBuf,
    pub upper: Utf8PathBuf,
    pub work: Utf8PathBuf,
    pub mount_at: String,
}

/// Abstraction over the hakoniwa container API, modeling the four
/// `run*` variants from `sandbox-stage.sh`.
pub struct SandboxRunner {
    sandbox_dir: Utf8PathBuf,
    /// Host directory bind-mounted read-write at /var/log inside the container.
    log_dir: Utf8PathBuf,
    /// Extra (host_path, container_path) read-write bind mounts.
    extra_rw: Vec<(Utf8PathBuf, String)>,
    /// Extra (host_path, container_path) read-only bind mounts.
    extra_ro: Vec<(Utf8PathBuf, String)>,
    /// Absolute path to the project directory, mounted read-only at /scripts.
    scripts_dir: Option<Utf8PathBuf>,
    /// Overlayfs mounts performed inside the container before each command.
    overlays: Vec<OverlaySpec>,
}

impl SandboxRunner {
    pub fn new(sandbox_dir: &Utf8Path, log_dir: Utf8PathBuf) -> Self {
        Self {
            sandbox_dir: sandbox_dir.to_path_buf(),
            log_dir,
            extra_rw: vec![],
            extra_ro: vec![],
            scripts_dir: None,
            overlays: vec![],
        }
    }

    /// Add an overlayfs mount to perform inside the sandbox.  Lower is
    /// bind-mounted read-only, upper/work read-write.  `mount_at` is
    /// created if needed.  Multiple overlays may be added; they are
    /// performed in registration order.
    pub fn with_overlay(mut self, spec: OverlaySpec) -> Self {
        self.overlays.push(spec);
        self
    }

    pub fn log_dir(&self) -> &Utf8Path {
        &self.log_dir
    }

    /// Bind-mount `target_dir` read-write at `/target` (for cross-emerge).
    pub fn with_target(mut self, target_dir: &Utf8Path) -> Self {
        self.extra_rw
            .push((target_dir.to_path_buf(), "/target".into()));
        self
    }

    /// Bind-mount `build_dir` read-write at `/build` (for kernel/bootloader builds).
    /// Also mounts `scripts_dir` read-only at `/scripts`.
    pub fn with_build(mut self, build_dir: &Utf8Path, scripts_dir: &Utf8Path) -> Self {
        self.extra_rw
            .push((build_dir.to_path_buf(), "/build".into()));
        self.scripts_dir = Some(scripts_dir.to_path_buf());
        self
    }

    /// Bind-mount `cache_dir` read-write at `/cache` (for source cache).
    pub fn with_cache(mut self, cache_dir: &Utf8Path) -> Self {
        self.extra_rw
            .push((cache_dir.to_path_buf(), "/cache".into()));
        self
    }

    /// Bind-mount `binpkgs_dir` read-write at `/binpkgs` so portage's
    /// `PKGDIR=/binpkgs` reads/writes a shared host cache.  Caller is
    /// expected to segment by (chost, cflags-hash) on the host side so
    /// boards with different toolchains don't share incompatible
    /// binaries.
    pub fn with_binpkgs(mut self, binpkgs_dir: &Utf8Path) -> Self {
        self.extra_rw
            .push((binpkgs_dir.to_path_buf(), "/binpkgs".into()));
        self
    }

    /// Run a shell command (via `bash --login -c`) inside the sandbox.
    pub fn run(&self, cmd: &str) -> Result<()> {
        let container = self.build_container();
        let full = format!("{}{}", self.overlay_prefix(), cmd);
        let mut command = container.command("/bin/bash");
        command
            .arg("--login")
            .arg("-c")
            .arg(&full)
            .env("HOME", "/root")
            .env(
                "TERM",
                &std::env::var("TERM").unwrap_or_else(|_| "xterm".into()),
            )
            .env("COLORTERM", &std::env::var("COLORTERM").unwrap_or_default())
            .env("NO_COLOR", &std::env::var("NO_COLOR").unwrap_or_default());
        check_status(command.status()?).map_err(|e| annotate_cmd(e, cmd))
    }

    /// Run a shell command and capture its trimmed stdout.
    pub fn run_output(&self, cmd: &str) -> Result<String> {
        let container = self.build_container();
        let full = format!("{}{}", self.overlay_prefix(), cmd);
        let mut command = container.command("/bin/bash");
        command
            .arg("--login")
            .arg("-c")
            .arg(&full)
            .env("HOME", "/root")
            .stdout(hakoniwa::Stdio::piped());
        let output = command.output()?;
        if !output.status.success() {
            return Err(crate::error::Error::CommandFailed {
                code: output.status.code,
                reason: format!("{cmd}: {}", output.status.reason),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Spawn an interactive `bash --login` shell in the sandbox.
    pub fn shell(&self) -> Result<()> {
        let container = self.build_container();
        let mut command = container.command("/bin/bash");
        let overlay = self.overlay_prefix();
        if overlay.is_empty() {
            command.arg("--login");
        } else {
            command
                .arg("-c")
                .arg(format!("{overlay}exec bash --login").as_str());
        }
        command
            .env("HOME", "/root")
            .env(
                "TERM",
                &std::env::var("TERM").unwrap_or_else(|_| "xterm".into()),
            )
            .env("COLORTERM", &std::env::var("COLORTERM").unwrap_or_default());
        check_status(command.status()?)
    }

    fn build_container(&self) -> Container {
        let _ = std::fs::create_dir_all(&self.log_dir);

        let mut c = Container::new();
        // Container::new() already unshares Mount, User, Pid.
        // Add the remaining namespaces to match --unshare-all.
        c.unshare(Namespace::Ipc)
            .unshare(Namespace::Uts)
            .unshare(Namespace::Cgroup)
            // Keep host network (--network=host).
            .share(Namespace::Network)
            .rootdir(&self.sandbox_dir)
            .runctl(Runctl::RootdirRW)
            .runctl(Runctl::AllowNewPrivs)
            .devfsmount("/dev")
            .bindmount_ro("/etc/resolv.conf", "/etc/resolv.conf")
            .tmpfsmount("/tmp")
            .tmpfsmount("/dev/shm")
            // Explicit bind mount so portage logs are always reachable at
            // <sandbox_dir>/var/log/ from the host.
            .bindmount_rw(self.log_dir.as_str(), "/var/log");
        // Map caller → root, plus subordinate IDs for portage user etc.
        c.uidmaps(&uid_maps());
        c.gidmaps(&gid_maps());

        for (host, cpath) in &self.extra_rw {
            c.bindmount_rw(host.as_str(), cpath);
        }
        for (host, cpath) in &self.extra_ro {
            c.bindmount_ro(host.as_str(), cpath);
        }
        if let Some(ref scripts) = self.scripts_dir {
            c.bindmount_ro(scripts.as_str(), "/scripts");
        }

        // Bind in each overlay's host dirs at hidden container paths.
        // The actual `mount -t overlay` happens inside the container via
        // overlay_prefix() since hakoniwa drops privileges before its
        // own bindmounts are visible.
        for (i, ovl) in self.overlays.iter().enumerate() {
            let _ = std::fs::create_dir_all(&ovl.lower);
            let _ = std::fs::create_dir_all(&ovl.upper);
            let _ = std::fs::create_dir_all(&ovl.work);
            c.bindmount_ro(ovl.lower.as_str(), &overlay_lower_path(i));
            c.bindmount_rw(ovl.upper.as_str(), &overlay_upper_path(i));
            c.bindmount_rw(ovl.work.as_str(), &overlay_work_path(i));
        }
        c
    }

    /// Shell prefix that mounts each registered overlay before the user
    /// command runs.  Empty when no overlays are configured.
    fn overlay_prefix(&self) -> String {
        if self.overlays.is_empty() {
            return String::new();
        }
        let mut parts = Vec::with_capacity(self.overlays.len());
        for (i, ovl) in self.overlays.iter().enumerate() {
            parts.push(format!(
                "mkdir -p {mp} && mount -t overlay overlay \
                 -o lowerdir={lo},upperdir={up},workdir={wk} {mp}",
                mp = ovl.mount_at,
                lo = overlay_lower_path(i),
                up = overlay_upper_path(i),
                wk = overlay_work_path(i),
            ));
        }
        // `set -e` so a failed overlay aborts before user code runs.
        format!("set -e; {}; ", parts.join("; "))
    }
}

fn overlay_lower_path(i: usize) -> String { format!("/.overlay/{i}/lower") }
fn overlay_upper_path(i: usize) -> String { format!("/.overlay/{i}/upper") }
fn overlay_work_path(i: usize) -> String { format!("/.overlay/{i}/work") }

/// Build UID maps: caller → root + subordinate range. Mirrors hakoniwa CLI `--userns=auto`.
fn uid_maps() -> Vec<(u32, u32, u32)> {
    let my_id = unsafe { libc::getuid() } as u32;
    idmaps_for(my_id, "/etc/subuid")
}

/// Build GID maps: caller → root + subordinate range. Mirrors hakoniwa CLI `--userns=auto`.
fn gid_maps() -> Vec<(u32, u32, u32)> {
    let my_id = unsafe { libc::getgid() } as u32;
    idmaps_for(my_id, "/etc/subgid")
}

fn idmaps_for(id: u32, subid_file: &str) -> Vec<(u32, u32, u32)> {
    let username = std::env::var("USER").unwrap_or_else(|_| id.to_string());
    let mut maps = vec![(0, id, 1)]; // container root → caller
    if let Some((sub_start, sub_count)) = read_subid(&username, id, subid_file) {
        maps.push((1, sub_start, sub_count));
    }
    maps
}

fn read_subid(user: &str, id: u32, path: &str) -> Option<(u32, u32)> {
    let id_str = id.to_string();
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() >= 3 && (parts[0] == user || parts[0] == id_str) {
            let start: u32 = parts[1].parse().ok()?;
            let count: u32 = parts[2].parse().ok()?;
            return Some((start, count));
        }
    }
    None
}

/// Remove a directory tree that may contain root-owned files from a stage3 unpack.
///
/// Runs `rm -rf` inside a container with the full subordinate uid/gid maps so
/// that uid 0 inside can access files owned by portage and other system users.
pub fn destroy_dir(dir: &Utf8Path, cache_base: &Utf8Path) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let dir_in_container = format!(
        "/cache/{}",
        dir.strip_prefix(cache_base)
            .unwrap_or(dir)
    );

    let mut container = Container::new();
    container
        .rootfs("/")?
        .unshare(Namespace::Ipc)
        .unshare(Namespace::Uts)
        .unshare(Namespace::Cgroup)
        .devfsmount("/dev")
        .tmpfsmount("/tmp")
        .tmpfsmount("/dev/shm")
        .bindmount_rw(cache_base.as_str(), "/cache")
        .runctl(Runctl::AllowNewPrivs);
    container.uidmaps(&uid_maps());
    container.gidmaps(&gid_maps());

    let cmd = format!("rm -rf {dir_in_container}");
    let mut command = container.command("/bin/sh");
    command.arg("-c").arg(&cmd);
    check_status(command.status()?).map_err(|e| annotate_cmd(e, &cmd))
}

/// Unpack a stage3 source tarball into `dest_dir`, preserving ownership and xattrs.
///
/// `source_stage` is the seed stage3 tarball (catalyst: `source_path`).
/// Runs inside a container rooted at the host `/` (so that tar, bash, etc.
/// are available from the host system).  The entire cache base directory is
/// bind-mounted read-write at `/cache` so that both the source tarball and
/// the destination directory are reachable inside the container.
pub fn unpack_tarball(source_stage: &Utf8Path, dest_dir: &Utf8Path, cache_base: &Utf8Path) -> Result<()> {
    std::fs::create_dir_all(dest_dir)?;

    // Paths inside the container: /cache/<relative_to_cache_base>
    let stage_in_container = format!(
        "/cache/{}",
        source_stage.strip_prefix(cache_base).unwrap_or(source_stage)
    );
    let dest_in_container = format!(
        "/cache/{}",
        dest_dir.strip_prefix(cache_base).unwrap_or(dest_dir)
    );

    let mut container = Container::new();
    container
        .rootfs("/")?
        .unshare(Namespace::Ipc)
        .unshare(Namespace::Uts)
        .unshare(Namespace::Cgroup)
        .devfsmount("/dev")
        .tmpfsmount("/tmp")
        .tmpfsmount("/dev/shm")
        .bindmount_rw(cache_base.as_str(), "/cache")
        .runctl(Runctl::AllowNewPrivs);
    container.uidmaps(&uid_maps());
    container.gidmaps(&gid_maps());

    let cmd = format!(
        "mkdir -p {dest} && \
         tar --overwrite -xpf {stage} \
           --xattrs-include='*.*' \
           --numeric-owner \
           --exclude='./dev' \
           -C {dest}",
        stage = stage_in_container,
        dest = dest_in_container,
    );

    let mut command = container.command("/bin/sh");
    command.arg("-c").arg(&cmd);
    check_status(command.status()?).map_err(|e| annotate_cmd(e, &cmd))
}

/// Pack a directory tree into a stage3-compatible tarball, preserving ownership and xattrs.
///
/// Runs inside a container rooted at the host `/` so that uid 0 inside can read files
/// owned by portage and other system users.  Both the source directory and the output
/// tarball are reachable via the `/cache` bind-mount.
///
/// `compression`: "xz" (default), "gz", or "none" (produces a `.tar`).
pub fn pack_tarball(
    src_dir: &Utf8Path,
    tarball: &Utf8Path,
    cache_base: &Utf8Path,
    compression: &str,
) -> Result<()> {
    let src_in_container = format!(
        "/cache/{}",
        src_dir.strip_prefix(cache_base).unwrap_or(src_dir)
    );
    let tarball_in_container = format!(
        "/cache/{}",
        tarball.strip_prefix(cache_base).unwrap_or(tarball)
    );

    if let Some(parent) = tarball.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let compress_flag = match compression {
        "gz" | "gzip" => "-z",
        "none" => "",
        _ => "-J", // xz
    };

    let cmd = format!(
        "tar -cp{compress} --xattrs --xattrs-include='*.*' --numeric-owner \
         --exclude='./dev' --exclude='./proc' --exclude='./sys' \
         --exclude='./run' --exclude='./tmp' \
         -f {tarball} -C {src} .",
        compress = compress_flag,
        tarball = tarball_in_container,
        src = src_in_container,
    );

    let mut container = Container::new();
    container
        .rootfs("/")?
        .unshare(Namespace::Ipc)
        .unshare(Namespace::Uts)
        .unshare(Namespace::Cgroup)
        .devfsmount("/dev")
        .tmpfsmount("/tmp")
        .tmpfsmount("/dev/shm")
        .bindmount_rw(cache_base.as_str(), "/cache")
        .runctl(Runctl::AllowNewPrivs);
    container.uidmaps(&uid_maps());
    container.gidmaps(&gid_maps());

    let mut command = container.command("/bin/sh");
    command.arg("-c").arg(&cmd);
    check_status(command.status()?).map_err(|e| annotate_cmd(e, &cmd))
}

/// Prefix a failed-command error with the command string for diagnostics.
fn annotate_cmd(e: crate::error::Error, cmd: &str) -> crate::error::Error {
    match e {
        crate::error::Error::CommandFailed { code, reason } => {
            crate::error::Error::CommandFailed {
                code,
                reason: format!("{cmd}: {reason}"),
            }
        }
        other => other,
    }
}
