use camino::{Utf8Path, Utf8PathBuf};
use hakoniwa::{Container, Namespace, Runctl};

use crate::error::{check_status, Result};

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
}

impl SandboxRunner {
    pub fn new(sandbox_dir: &Utf8Path, log_dir: Utf8PathBuf) -> Self {
        Self {
            sandbox_dir: sandbox_dir.to_path_buf(),
            log_dir,
            extra_rw: vec![],
            extra_ro: vec![],
            scripts_dir: None,
        }
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

    /// Run a shell command (via `bash --login -c`) inside the sandbox.
    pub fn run(&self, cmd: &str) -> Result<()> {
        let container = self.build_container();
        let mut command = container.command("/bin/bash");
        command
            .arg("--login")
            .arg("-c")
            .arg(cmd)
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
        let mut command = container.command("/bin/bash");
        command
            .arg("--login")
            .arg("-c")
            .arg(cmd)
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
        command
            .arg("--login")
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
            .tmpfsmount("/tmp")
            .tmpfsmount("/dev/shm")
            // Explicit bind mount so portage logs are always reachable at
            // <sandbox_dir>/var/log/ from the host.
            .bindmount_rw(self.log_dir.as_str(), "/var/log");

        // On systemd systems /etc/resolv.conf is typically a symlink to
        // /run/systemd/resolve/stub-resolv.conf, which doesn't exist inside
        // the sandbox chroot.  When the symlink target isn't reachable inside
        // the container root, fall back to a static resolv.conf (1.1.1.1).
        let host_resolv = Utf8Path::new("/etc/resolv.conf");
        if host_resolv.is_file() && can_bindmount_resolv(&self.sandbox_dir) {
            c.bindmount_ro("/etc/resolv.conf", "/etc/resolv.conf");
        } else {
            let sandbox_resolv = self.sandbox_dir.join("etc/resolv.conf");
            let _ = std::fs::create_dir_all(sandbox_resolv.parent().unwrap());
            std::fs::write(
                &sandbox_resolv,
                "nameserver 1.1.1.1\nnameserver 2606:4700:4700::1111\n",
            )
            .expect("failed to write fallback resolv.conf");
        }
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
        c
    }
}

/// Check whether the host `/etc/resolv.conf` can be bind-mounted into a
/// container rooted at `sandbox_dir`.  On systemd hosts it's a symlink to
/// `/run/systemd/resolve/stub-resolv.conf`; the bind mount will fail with
/// EPERM unless the symlink target also exists inside the sandbox.
fn can_bindmount_resolv(sandbox_dir: &Utf8Path) -> bool {
    let host_resolv = std::path::Path::new("/etc/resolv.conf");
    match host_resolv.canonicalize() {
        Ok(real) => {
            let real_str = match real.to_str() {
                Some(s) => s,
                None => return false,
            };
            // If the canonical path starts with /run/systemd, the target
            // won't exist inside the sandbox — fall back to static DNS.
            if real_str.starts_with("/run/systemd") {
                return false;
            }
            // Otherwise check that the target exists inside the sandbox.
            let stripped = real_str.trim_start_matches('/');
            sandbox_dir.join(stripped).is_file()
        }
        Err(_) => false,
    }
}

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
    let dir_in_container = format!("/cache/{}", dir.strip_prefix(cache_base).unwrap_or(dir));

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
pub fn unpack_tarball(
    source_stage: &Utf8Path,
    dest_dir: &Utf8Path,
    cache_base: &Utf8Path,
) -> Result<()> {
    std::fs::create_dir_all(dest_dir)?;

    // Paths inside the container: /cache/<relative_to_cache_base>
    let stage_in_container = format!(
        "/cache/{}",
        source_stage
            .strip_prefix(cache_base)
            .unwrap_or(source_stage)
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
        crate::error::Error::CommandFailed { code, reason } => crate::error::Error::CommandFailed {
            code,
            reason: format!("{cmd}: {reason}"),
        },
        other => other,
    }
}
