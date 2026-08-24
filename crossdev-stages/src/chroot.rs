//! A shell inside the image itself, running the board's own binaries.
//!
//! `enter` opens a shell in the container the *build* runs in: the build host,
//! with a cross compiler and the target mounted at /target.  This opens one in
//! the *target* -- the filesystem that will be on the card -- where `/bin/bash`
//! is a riscv64 or aarch64 executable and runs through qemu-user.
//!
//! No qemu is copied into the rootfs.  Linux' binfmt_misc handlers are
//! registered with the `F` flag, which opens the interpreter at registration
//! time and keeps the file descriptor in the kernel, so it stays reachable from
//! inside a mount namespace that cannot see the host's /usr.  That is what the
//! flag exists for, and it is why a chroot needs nothing added to it.

use camino::{Utf8Path, Utf8PathBuf};
use hakoniwa::{Container, Namespace, Runctl};

use crate::error::{check_status, Error, Result};

/// The binfmt_misc handler that would run this architecture's binaries.
fn binfmt_handler(arch: &str) -> Option<&'static str> {
    Some(match arch {
        "riscv64" => "qemu-riscv64",
        "riscv32" => "qemu-riscv32",
        "aarch64" => "qemu-aarch64",
        "armv7a" | "armv6j" | "arm" => "qemu-arm",
        "i586" | "i686" => "qemu-i386",
        "x86_64" => return None, // native; nothing to emulate
        _ => return None,
    })
}

/// Whether the kernel will run this architecture's binaries for us, and
/// whether it will still do so once we are inside a mount namespace.
enum Emulation {
    /// Native, or a handler registered with `F`: nothing to arrange.
    Ready,
    /// Registered, but without `F`, so the interpreter has to be visible from
    /// inside the chroot -- which it is not.
    NeedsFixBinary(String),
    NotRegistered(String),
}

fn emulation_for(arch: &str) -> Emulation {
    let Some(handler) = binfmt_handler(arch) else {
        return Emulation::Ready;
    };
    let path = Utf8PathBuf::from("/proc/sys/fs/binfmt_misc").join(handler);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Emulation::NotRegistered(handler.to_string());
    };
    let flags = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("flags:"))
        .unwrap_or("");
    if flags.contains('F') {
        Emulation::Ready
    } else {
        Emulation::NeedsFixBinary(handler.to_string())
    }
}

/// Open a shell in a rootfs whose binaries are not this machine's.
pub fn run(rootfs: &Utf8Path, arch: &str, cmd: &[String]) -> Result<()> {
    if !rootfs.join("bin/sh").exists() && !rootfs.join("bin/bash").exists() {
        return Err(Error::CommandFailed {
            code: 1,
            reason: format!("{rootfs} has no shell in it yet"),
        });
    }

    match emulation_for(arch) {
        Emulation::Ready => {}
        Emulation::NeedsFixBinary(handler) => {
            return Err(Error::CommandFailed {
                code: 1,
                reason: format!(
                    "binfmt_misc handler {handler} is registered without the F flag, \
                     so its interpreter is not reachable from inside a chroot.  \
                     Re-register it with F (qemu's own binfmt.d files do), or run \
                     the binary directly: qemu-{arch}-static -L {rootfs} <program>"
                ),
            });
        }
        Emulation::NotRegistered(handler) => {
            return Err(Error::CommandFailed {
                code: 1,
                reason: format!(
                    "no binfmt_misc handler {handler}: the kernel cannot run {arch} \
                     binaries here.  Install qemu-user-static and register it \
                     (Debian: apt install qemu-user-static binfmt-support), or run \
                     the binary directly: qemu-{arch}-static -L {rootfs} <program>"
                ),
            });
        }
    }

    let mut c = Container::new();
    c.unshare(Namespace::Ipc)
        .unshare(Namespace::Uts)
        .unshare(Namespace::Cgroup)
        // Keep host networking, the same choice the build sandbox makes.
        .share(Namespace::Network)
        .rootdir(rootfs.as_str())
        .runctl(Runctl::RootdirRW)
        .runctl(Runctl::AllowNewPrivs)
        .devfsmount("/dev")
        .tmpfsmount("/tmp")
        .tmpfsmount("/dev/shm");
    c.uidmaps(&crate::container::uid_maps());
    c.gidmaps(&crate::container::gid_maps());

    // /etc/resolv.conf is the image's own; leave it alone.  A chroot into a
    // board image is for looking at what is there, and rewriting its config
    // would be changing the thing under inspection.

    let shell = if rootfs.join("bin/bash").exists() {
        "/bin/bash"
    } else {
        "/bin/sh"
    };
    let mut command = c.command(shell);
    if cmd.is_empty() {
        println!("{rootfs} ({arch}) -- binaries run through qemu-user");
        command.arg("-l");
    } else {
        command.arg("-lc").arg(cmd.join(" ").as_str());
    }
    command
        .env("HOME", "/root")
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env(
            "TERM",
            &std::env::var("TERM").unwrap_or_else(|_| "xterm".into()),
        );
    check_status(command.status()?)
}

#[cfg(test)]
mod tests {
    use super::binfmt_handler;

    #[test]
    fn each_arch_names_the_handler_that_would_run_it() {
        assert_eq!(binfmt_handler("riscv64"), Some("qemu-riscv64"));
        assert_eq!(binfmt_handler("armv7a"), Some("qemu-arm"));
        assert_eq!(binfmt_handler("i586"), Some("qemu-i386"));
    }

    #[test]
    fn the_host_architecture_needs_no_handler() {
        assert_eq!(binfmt_handler("x86_64"), None);
    }
}
