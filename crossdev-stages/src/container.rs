use camino::{Utf8Path, Utf8PathBuf};
use hakoniwa::{Container, Namespace, Runctl};

use crate::error::{check_status, Error, Result};
use crate::workspace::Workspace;

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
            // Bind the dedicated host log dir at /var/log inside the container.
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

/// A stale bind mount left behind after a crashed hakoniwa sandbox session.
#[derive(Debug, Clone)]
pub struct StaleMount {
    pub sandbox: String,
    pub path: Utf8PathBuf,
}

/// Paths inside a sandbox tree that `SandboxRunner` may bind-mount.
fn sandbox_bind_targets(_sandbox_dir: &Utf8Path) -> [&'static str; 2] {
    ["var/log", "etc/resolv.conf"]
}

/// Return true when `path` is a mount point in the current mount namespace.
pub fn is_mount_point(path: &Utf8Path) -> bool {
    let Ok(canonical) = path.canonicalize_utf8() else {
        return false;
    };
    let Ok(content) = std::fs::read_to_string("/proc/self/mountinfo") else {
        return false;
    };
    content.lines().any(|line| {
        mount_point_from_mountinfo(line)
            .and_then(|mp| mp.canonicalize_utf8().ok())
            .is_some_and(|mp| mp == canonical)
    })
}

fn mount_point_from_mountinfo(line: &str) -> Option<Utf8PathBuf> {
    let before = line.split(" - ").next()?;
    let mut fields = before.split_whitespace();
    fields.next()?; // mount id
    fields.next()?; // parent id
    fields.next()?; // major:minor
    fields.next()?; // root
    let mount_point = fields.next()?;
    Some(Utf8PathBuf::from(mount_point))
}

/// Find stale hakoniwa bind mounts under workspace sandboxes.
pub fn find_stale_sandbox_mounts(ws: &Workspace) -> Result<Vec<StaleMount>> {
    let mut stale = Vec::new();
    for sandbox_dir in ws.list_sandboxes()? {
        let name = sandbox_dir
            .file_name()
            .unwrap_or("?")
            .to_string();
        for sub in sandbox_bind_targets(&sandbox_dir) {
            let target = sandbox_dir.join(sub);
            if target.exists() && is_mount_point(&target) {
                stale.push(StaleMount {
                    sandbox: name.clone(),
                    path: target,
                });
            }
        }
    }
    Ok(stale)
}

/// Lazy-unmount stale sandbox bind mounts so cleanup and the next container run succeed.
pub fn recover_sandbox_mounts(ws: &Workspace, dry_run: bool) -> Result<usize> {
    let stale = find_stale_sandbox_mounts(ws)?;
    let mut count = 0usize;
    for mount in stale {
        if dry_run {
            println!(
                "Would unmount {} (sandbox {})",
                mount.path, mount.sandbox
            );
        } else {
            umount_lazy(&mount.path)?;
            restore_mount_target(&mount.path)?;
            println!("Unmounted {} (sandbox {})", mount.path, mount.sandbox);
        }
        count += 1;
    }
    Ok(count)
}

/// Unmount stale bind targets under `path` before removing a workspace entry.
pub fn recover_mounts_for_removal(path: &Utf8Path, dry_run: bool) -> Result<usize> {
    let mut count = 0usize;
    if path.is_dir() {
        for sub in sandbox_bind_targets(path) {
            let target = path.join(sub);
            if target.exists() && is_mount_point(&target) {
                if dry_run {
                    println!("Would unmount {}", target);
                } else {
                    umount_lazy(&target)?;
                    restore_mount_target(&target)?;
                    println!("Unmounted {}", target);
                }
                count += 1;
            }
        }
    }
    if path.exists() && is_mount_point(path) {
        if dry_run {
            println!("Would unmount {}", path);
        } else {
            umount_lazy(path)?;
            restore_mount_target(path)?;
            println!("Unmounted {}", path);
        }
        count += 1;
    }
    Ok(count)
}

fn umount_lazy(path: &Utf8Path) -> Result<()> {
    let cpath = std::ffi::CString::new(path.as_str())
        .map_err(|_| Error::CommandFailed {
            code: 1,
            reason: format!("mount path contains NUL byte: {path}"),
        })?;
    let rc = unsafe { libc::umount2(cpath.as_ptr(), libc::MNT_DETACH) };
    if rc == 0 {
        return Ok(());
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::EINVAL) | Some(libc::ENOENT) => Ok(()),
        Some(code) => Err(Error::CommandFailed {
            code,
            reason: format!("umount2 {path}"),
        }),
        None => Err(Error::CommandFailed {
            code: 1,
            reason: format!("umount2 {path}"),
        }),
    }
}

/// Ensure bind-mount targets exist after a lazy unmount.
fn restore_mount_target(path: &Utf8Path) -> Result<()> {
    if path.ends_with("var/log") {
        std::fs::create_dir_all(path)?;
    } else if path.ends_with("resolv.conf") && !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            path,
            "nameserver 1.1.1.1\nnameserver 2606:4700:4700::1111\n",
        )?;
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proc_is_a_mount_point() {
        assert!(is_mount_point(Utf8Path::new("/proc")));
    }

    #[test]
    fn regular_dir_is_not_a_mount_point() {
        let dir = std::env::temp_dir().join("crossdev-stages-mount-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = Utf8PathBuf::try_from(dir).unwrap();
        assert!(!is_mount_point(&path));
        let _ = std::fs::remove_dir(&path);
    }
}
