//! Rootfs provider seam.
//!
//! The image pipeline (checkout, kernel, bootloader, assemble mechanics,
//! pack) is provider-agnostic; the provider decides how `/target` is
//! seeded, what the `deps` step installs, and how the OS inside the
//! image is configured.  Selected per board via `ROOTFS_PROVIDER` in
//! board.conf; absent means Gentoo, keeping every existing board
//! behavior-identical.

/// Who fills and configures the image's root filesystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RootfsProvider {
    /// Gentoo stage3 seed; the `deps` step cross-emerges the default and
    /// board target-package lists; `assemble` writes OpenRC config.
    #[default]
    Gentoo,
    /// Debian rootfs built by debootstrap inside the sandbox during the
    /// `deps` step; `assemble` writes systemd config.  The foreign-arch
    /// second stage runs in a chroot, which needs qemu-user binfmt (with
    /// the F flag) registered on the host.
    Debian,
    /// Nothing is seeded, installed, or configured — board hooks
    /// (`override-deps.sh`, `post-assemble.sh`, …) fill the rootfs.
    None,
}

impl RootfsProvider {
    /// Parse a `ROOTFS_PROVIDER` board.conf value.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "gentoo" => Some(Self::Gentoo),
            "debian" => Some(Self::Debian),
            "none" => Some(Self::None),
            _ => Option::None,
        }
    }

    /// Whether image builds need the crossdev toolchain store.  Gentoo
    /// always does (`deps` cross-emerges into /target); other providers
    /// only when a step compiles target code.
    pub fn needs_cross_toolchain(&self, steps: &[&str]) -> bool {
        match self {
            Self::Gentoo => true,
            Self::Debian | Self::None => steps
                .iter()
                .any(|s| matches!(*s, "kernel" | "bootloader")),
        }
    }

    /// Whether `/target` is seeded from a Gentoo stage3 tarball.
    pub fn provisions_stage3(&self) -> bool {
        matches!(self, Self::Gentoo)
    }

    /// The board.conf value for this provider; also recorded as the
    /// target dir's `.provider` marker so targets are never shared
    /// across providers.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Gentoo => "gentoo",
            Self::Debian => "debian",
            Self::None => "none",
        }
    }
}

/// Debian's name for a Gentoo-style arch string.  Debian's i386 port
/// requires i686, so sub-i686 x86 boards have no Debian port.
pub fn debian_arch(arch: &str) -> Option<&'static str> {
    match arch {
        "riscv64" => Some("riscv64"),
        "aarch64" => Some("arm64"),
        "x86_64" => Some("amd64"),
        "i686" => Some("i386"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::RootfsProvider;

    #[test]
    fn parse_known_values() {
        assert_eq!(RootfsProvider::parse("gentoo"), Some(RootfsProvider::Gentoo));
        assert_eq!(RootfsProvider::parse("debian"), Some(RootfsProvider::Debian));
        assert_eq!(RootfsProvider::parse("none"), Some(RootfsProvider::None));
        assert_eq!(RootfsProvider::parse("fedora"), Option::None);
    }

    #[test]
    fn default_is_gentoo() {
        assert_eq!(RootfsProvider::default(), RootfsProvider::Gentoo);
    }

    #[test]
    fn name_round_trips_through_parse() {
        for p in [RootfsProvider::Gentoo, RootfsProvider::None] {
            assert_eq!(RootfsProvider::parse(p.name()), Some(p));
        }
    }

    #[test]
    fn gentoo_always_needs_toolchain() {
        assert!(RootfsProvider::Gentoo.needs_cross_toolchain(&["assemble", "pack"]));
    }

    #[test]
    fn none_needs_toolchain_only_for_compiled_steps() {
        let p = RootfsProvider::None;
        assert!(!p.needs_cross_toolchain(&["deps", "assemble", "pack"]));
        assert!(p.needs_cross_toolchain(&["kernel", "assemble", "pack"]));
        assert!(p.needs_cross_toolchain(&["bootloader", "pack"]));
    }

    #[test]
    fn debian_arch_mapping() {
        assert_eq!(super::debian_arch("riscv64"), Some("riscv64"));
        assert_eq!(super::debian_arch("aarch64"), Some("arm64"));
        assert_eq!(super::debian_arch("x86_64"), Some("amd64"));
        assert_eq!(super::debian_arch("i686"), Some("i386"));
        assert_eq!(super::debian_arch("i586"), Option::None);
        assert_eq!(super::debian_arch("riscv32"), Option::None);
    }
}
