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
    /// `deps` step; `assemble` writes systemd config.
    Debian,
    /// Ubuntu rootfs, debootstrap again.  Differs from Debian in the
    /// keyring, the mirror (everything but amd64/i386 lives on
    /// ports.ubuntu.com) and in having no rolling suite alias.
    Ubuntu,
    /// Nothing is seeded, installed, or configured; board hooks
    /// (`override-deps.sh`, `post-assemble.sh`, ...) fill the rootfs.
    None,
}

impl RootfsProvider {
    /// Parse a `ROOTFS_PROVIDER` board.conf value.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "gentoo" => Some(Self::Gentoo),
            "debian" => Some(Self::Debian),
            "ubuntu" => Some(Self::Ubuntu),
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
            Self::Debian | Self::Ubuntu | Self::None => steps
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
    /// across providers, and the prefix of the board's extra-package
    /// list (`debian-packages.txt`, `ubuntu-packages.txt`).
    pub fn name(&self) -> &'static str {
        match self {
            Self::Gentoo => "gentoo",
            Self::Debian => "debian",
            Self::Ubuntu => "ubuntu",
            Self::None => "none",
        }
    }

    /// The debootstrap flavour behind this provider, if it is one.
    pub fn debootstrap(&self) -> Option<Debootstrap> {
        match self {
            Self::Debian => Some(Debootstrap::DEBIAN),
            Self::Ubuntu => Some(Debootstrap::UBUNTU),
            Self::Gentoo | Self::None => None,
        }
    }
}

/// When the `debootstrap --foreign` second stage runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SecondStage {
    /// In a chroot on the build host, at `deps` time.  Executes
    /// target-arch binaries, so a foreign arch needs qemu-user binfmt
    /// registered with the F flag.  The default, and what every other
    /// image builder does (Armbian, Raspberry Pi OS, debos, vmdb2): the
    /// image that ships is a finished system.
    #[default]
    Chroot,
    /// On the board, at first boot.  `deps` stops after stage 1 and
    /// `assemble` installs a `/sbin/init` shim that finishes the
    /// bootstrap and hands over to the real init.
    ///
    /// Opt-in, for a build host that cannot register qemu-user binfmt.
    /// It costs: the image ships every .deb stage 1 downloaded (roughly
    /// double the size until the shim's `apt-get clean` runs), and the
    /// first boot spends minutes unpacking and configuring hundreds of
    /// packages on the board, over a serial console, where a failure
    /// leaves a half-configured system someone has to repair by hand.
    FirstBoot,
}

impl SecondStage {
    /// Parse a `ROOTFS_SECOND_STAGE` board.conf value.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "chroot" => Some(Self::Chroot),
            "first-boot" => Some(Self::FirstBoot),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Chroot => "chroot",
            Self::FirstBoot => "first-boot",
        }
    }
}

/// What one debootstrap-based provider needs that the other does not.
#[derive(Debug, Clone, Copy)]
pub struct Debootstrap {
    /// Portage atom providing `keyring`.
    pub keyring_pkg: &'static str,
    /// Absolute path of the archive keyring inside the sandbox.  Passed
    /// as `--keyring=` so signature checking never depends on the suite
    /// script picking it up (Ubuntu's does that only after an online
    /// end-of-life lookup, and falls back to the removed-keys keyring
    /// and the old-releases mirror when the lookup fails).
    pub keyring: &'static str,
    /// board.conf key naming the suite.
    pub suite_key: &'static str,
    /// board.conf key naming the mirror.
    pub mirror_key: &'static str,
    /// Suite used when the board names none, if the distribution has a
    /// rolling alias worth defaulting to.  Ubuntu has none, so an
    /// Ubuntu board must name a codename.
    pub default_suite: Option<&'static str>,
}

impl Debootstrap {
    const DEBIAN: Self = Self {
        keyring_pkg: "app-crypt/debian-archive-keyring",
        keyring: "/usr/share/keyrings/debian-archive-keyring.gpg",
        suite_key: "DEBIAN_SUITE",
        mirror_key: "DEBIAN_MIRROR",
        default_suite: Some("stable"),
    };

    const UBUNTU: Self = Self {
        keyring_pkg: "app-crypt/ubuntu-keyring",
        keyring: "/usr/share/keyrings/ubuntu-archive-keyring.gpg",
        suite_key: "UBUNTU_SUITE",
        mirror_key: "UBUNTU_MIRROR",
        default_suite: None,
    };

    /// Aliases that move under the build's feet; a board wanting a
    /// reproducible image names a codename instead.
    pub fn is_moving_alias(&self, suite: &str) -> bool {
        matches!(suite, "stable" | "testing" | "unstable" | "oldstable")
    }

    /// Mirror to bootstrap from when the board names none.  Ubuntu keeps
    /// only amd64 and i386 on archive.ubuntu.com; every other port is on
    /// ports.ubuntu.com, under its own `/ubuntu-ports` root.
    pub fn default_mirror(&self, dpkg_arch: &str) -> &'static str {
        match (self.suite_key, dpkg_arch) {
            ("UBUNTU_SUITE", "amd64" | "i386") => "http://archive.ubuntu.com/ubuntu",
            ("UBUNTU_SUITE", _) => "http://ports.ubuntu.com/ubuntu-ports",
            _ => "https://deb.debian.org/debian",
        }
    }
}

/// dpkg's name for a Gentoo-style arch string.  Debian's i386 port
/// requires i686, so sub-i686 x86 boards have no Debian port.
pub fn dpkg_arch(arch: &str) -> Option<&'static str> {
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
    use super::{Debootstrap, RootfsProvider, SecondStage};

    #[test]
    fn parse_known_values() {
        assert_eq!(RootfsProvider::parse("gentoo"), Some(RootfsProvider::Gentoo));
        assert_eq!(RootfsProvider::parse("debian"), Some(RootfsProvider::Debian));
        assert_eq!(RootfsProvider::parse("ubuntu"), Some(RootfsProvider::Ubuntu));
        assert_eq!(RootfsProvider::parse("none"), Some(RootfsProvider::None));
        assert_eq!(RootfsProvider::parse("fedora"), Option::None);
    }

    #[test]
    fn default_is_gentoo() {
        assert_eq!(RootfsProvider::default(), RootfsProvider::Gentoo);
    }

    #[test]
    fn name_round_trips_through_parse() {
        for p in [
            RootfsProvider::Gentoo,
            RootfsProvider::Debian,
            RootfsProvider::Ubuntu,
            RootfsProvider::None,
        ] {
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
    fn dpkg_arch_mapping() {
        assert_eq!(super::dpkg_arch("riscv64"), Some("riscv64"));
        assert_eq!(super::dpkg_arch("aarch64"), Some("arm64"));
        assert_eq!(super::dpkg_arch("x86_64"), Some("amd64"));
        assert_eq!(super::dpkg_arch("i686"), Some("i386"));
        assert_eq!(super::dpkg_arch("i586"), Option::None);
        assert_eq!(super::dpkg_arch("riscv32"), Option::None);
    }

    #[test]
    fn only_debootstrap_providers_carry_a_flavour() {
        assert!(RootfsProvider::Debian.debootstrap().is_some());
        assert!(RootfsProvider::Ubuntu.debootstrap().is_some());
        assert!(RootfsProvider::Gentoo.debootstrap().is_none());
        assert!(RootfsProvider::None.debootstrap().is_none());
    }

    #[test]
    fn ubuntu_ports_everything_but_x86() {
        let u = Debootstrap::UBUNTU;
        assert_eq!(u.default_mirror("amd64"), "http://archive.ubuntu.com/ubuntu");
        assert_eq!(u.default_mirror("i386"), "http://archive.ubuntu.com/ubuntu");
        assert_eq!(
            u.default_mirror("arm64"),
            "http://ports.ubuntu.com/ubuntu-ports"
        );
        assert_eq!(
            u.default_mirror("riscv64"),
            "http://ports.ubuntu.com/ubuntu-ports"
        );
        assert_eq!(
            Debootstrap::DEBIAN.default_mirror("riscv64"),
            "https://deb.debian.org/debian"
        );
    }

    #[test]
    fn ubuntu_has_no_default_suite() {
        assert_eq!(Debootstrap::DEBIAN.default_suite, Some("stable"));
        assert_eq!(Debootstrap::UBUNTU.default_suite, None);
    }

    #[test]
    fn second_stage_defaults_to_chroot() {
        assert_eq!(SecondStage::default(), SecondStage::Chroot);
        assert_eq!(SecondStage::parse("chroot"), Some(SecondStage::Chroot));
        assert_eq!(
            SecondStage::parse("first-boot"),
            Some(SecondStage::FirstBoot)
        );
        assert_eq!(SecondStage::parse("later"), None);
        for s in [SecondStage::Chroot, SecondStage::FirstBoot] {
            assert_eq!(SecondStage::parse(s.name()), Some(s));
        }
    }
}
