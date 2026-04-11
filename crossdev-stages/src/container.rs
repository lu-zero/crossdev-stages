use std::path::{Path, PathBuf};

use hakoniwa::{Container, Namespace, Runctl};

use crate::error::{check_status, Result};

/// Abstraction over the hakoniwa container API, modeling the four
/// `run*` variants from `sandbox-stage.sh`.
pub struct SandboxRunner {
    sandbox_dir: PathBuf,
    /// Extra (host_path, container_path) read-write bind mounts.
    extra_rw: Vec<(PathBuf, String)>,
    /// Extra (host_path, container_path) read-only bind mounts.
    extra_ro: Vec<(PathBuf, String)>,
    /// Absolute path to the project directory, mounted read-only at /scripts.
    scripts_dir: Option<PathBuf>,
    /// Sysroot bind-mount: (host_path, /usr/$chost).
    sysroot: Option<(PathBuf, String)>,
}

impl SandboxRunner {
    pub fn new(sandbox_dir: &Path) -> Self {
        Self {
            sandbox_dir: sandbox_dir.to_path_buf(),
            extra_rw: vec![],
            extra_ro: vec![],
            scripts_dir: None,
            sysroot: None,
        }
    }

    /// Bind-mount `target_dir` read-write at `/target` (for cross-emerge).
    pub fn with_target(mut self, target_dir: &Path) -> Self {
        self.extra_rw
            .push((target_dir.to_path_buf(), "/target".into()));
        self
    }

    /// Set the sysroot to be bind-mounted at `/usr/$chost`.
    /// All subsequent run calls will include this mount.
    pub fn with_sysroot(mut self, sysroot_dir: &Path, chost: &str) -> Self {
        self.sysroot = Some((sysroot_dir.to_path_buf(), format!("/usr/{chost}")));
        self
    }

    /// Bind-mount `build_dir` read-write at `/build` (for kernel/bootloader builds).
    /// Also mounts `scripts_dir` read-only at `/scripts`.
    pub fn with_build(mut self, build_dir: &Path, scripts_dir: &Path) -> Self {
        self.extra_rw
            .push((build_dir.to_path_buf(), "/build".into()));
        self.scripts_dir = Some(scripts_dir.to_path_buf());
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
        check_status(command.status()?)
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
                reason: output.status.reason,
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
            .tmpfsmount("/dev/shm");
        // Map caller → root, plus subordinate IDs for portage user etc.
        c.uidmaps(&uid_maps());
        c.gidmaps(&gid_maps());

        for (host, cpath) in &self.extra_rw {
            c.bindmount_rw(host.to_str().unwrap_or_default(), cpath);
        }
        for (host, cpath) in &self.extra_ro {
            c.bindmount_ro(host.to_str().unwrap_or_default(), cpath);
        }
        if let Some(ref scripts) = self.scripts_dir {
            c.bindmount_ro(scripts.to_str().unwrap_or_default(), "/scripts");
        }
        if let Some((ref host_path, ref container_path)) = self.sysroot {
            c.bindmount_rw(host_path.to_str().unwrap_or_default(), container_path);
        }
        c
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

/// Unpack a stage3 tarball into `dest_dir`, preserving ownership and xattrs.
///
/// Runs inside a container rooted at the host `/` (so that tar, bash, etc.
/// are available from the host system).  The entire cache base directory is
/// bind-mounted read-write at `/cache` so that both the source tarball and
/// the destination directory are reachable inside the container.
pub fn unpack_tarball(stage_file: &Path, dest_dir: &Path, cache_base: &Path) -> Result<()> {
    std::fs::create_dir_all(dest_dir)?;

    let cache_str = cache_base.to_str().unwrap_or_default();
    // Paths inside the container: /cache/<relative_to_cache_base>
    let stage_in_container = format!(
        "/cache/{}",
        stage_file
            .strip_prefix(cache_base)
            .unwrap_or(stage_file)
            .to_str()
            .unwrap_or_default()
    );
    let dest_in_container = format!(
        "/cache/{}",
        dest_dir
            .strip_prefix(cache_base)
            .unwrap_or(dest_dir)
            .to_str()
            .unwrap_or_default()
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
        .bindmount_rw(cache_str, "/cache")
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
    check_status(command.status()?)
}
