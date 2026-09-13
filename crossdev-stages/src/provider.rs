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
    /// Alpine rootfs unpacked by `apk` during the `deps` step, emerged
    /// from the crossdev-stages overlay's `app-arch/apk-tools`.
    /// The only provider that populates a foreign-arch root without
    /// executing a single target binary: apk runs on the host arch and
    /// `--no-scripts` (upstream's own answer for "extracting a system
    /// image for different architecture on alternative ROOT") keeps the
    /// packages' shell scriptlets from ever being exec'd.  No qemu, no
    /// binfmt.  `assemble` writes OpenRC config.
    Alpine,
    /// Fedora rootfs unpacked from a published container base image
    /// during the `deps` step, with `fedora-packages.txt` installed on
    /// top by dnf5; `assemble` writes systemd config.
    ///
    /// The only provider whose emulation cost is conditional.  Unpacking
    /// the image executes nothing, so a board that asks for no packages
    /// builds on a host with no binfmt at all.  Installing does need
    /// qemu-user binfmt, like the debian provider: rpm runs scriptlets
    /// inside the installroot.
    ///
    /// dnf5 is not in ::gentoo; the crossdev-stages overlay carries it along
    /// with dev-libs/libsolv and dev-libs/librepo, the rest of the
    /// closure being in the tree already.
    Fedora,
    /// Buildroot: a source build system, not a package archive.  The
    /// `deps` step clones buildroot, runs its defconfig and its build,
    /// and unpacks the `rootfs.tar` it emits into `/target`; buildroot
    /// configures its own OS, so `assemble` writes nothing.
    ///
    /// ## The two-toolchain question
    ///
    /// Buildroot builds its own cross toolchain, and this project exists
    /// to build one with crossdev and keep it in the store.  Two
    /// toolchains for one board is either waste or a contradiction, and
    /// there are three ways out:
    ///
    /// (a) buildroot builds everything, crossdev is skipped;
    /// (b) buildroot is pointed at the store prefix through
    ///     `BR2_TOOLCHAIN_EXTERNAL`;
    /// (c) buildroot supplies only the rootfs, this project keeps
    ///     building the kernel and bootloader.
    ///
    /// What is implemented is (a), and (a) and (c) are the same code:
    /// the provider itself never asks for the toolchain, so a board
    /// picks between them with `BUILD_STEPS` alone.  `(deps assemble
    /// pack)` is (a) -- buildroot's own kernel and bootloader, no
    /// crossdev.  Adding `checkout kernel bootloader` back is (c), and
    /// then two toolchains really are built, which is a cost a board has
    /// to mean to pay.
    ///
    /// (b) is the end state this design points at and is not here.  It
    /// is not a flag: buildroot checks an external toolchain against
    /// what the defconfig claims about it -- libc, gcc version series,
    /// kernel-headers series, sysroot layout, C++ support -- so the
    /// provider would have to generate those symbols from the store key
    /// and probe the prefix for the rest, and be wrong loudly rather
    /// than late.  It also leaves CFLAGS with two owners:
    /// `BOARD_CFLAGS` builds the prefix (and so glibc), while
    /// `BR2_TARGET_OPTIMIZATION` builds everything buildroot compiles on
    /// top of it.  Worth doing, not worth faking.
    ///
    /// What (a) costs today: with no crossdev prefix there is no
    /// `{CROSS_COMPILE}readelf` and no `-gcc` to compile a probe with,
    /// so the ABI and ISA checks after `assemble` warn instead of
    /// running.  (b) would hand both of them back, which is the
    /// strongest argument for it.
    Buildroot,
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
            "alpine" => Some(Self::Alpine),
            "fedora" => Some(Self::Fedora),
            "buildroot" => Some(Self::Buildroot),
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
            Self::Debian
            | Self::Ubuntu
            | Self::Alpine
            | Self::Fedora
            | Self::Buildroot
            | Self::None => steps
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
            Self::Alpine => "alpine",
            Self::Fedora => "fedora",
            Self::Buildroot => "buildroot",
            Self::None => "none",
        }
    }

    /// Whether the `deps` step emerges atoms the crossdev-stages overlay
    /// carries (`app-arch/apk-tools`, `sys-apps/dnf5`).  The only reason
    /// any build needs the overlay repository to be reachable, so nothing
    /// else may treat it as a precondition.
    pub fn needs_overlay(&self) -> bool {
        match self {
            Self::Alpine | Self::Fedora => true,
            Self::Gentoo
            | Self::Debian
            | Self::Ubuntu
            | Self::Buildroot
            | Self::None => false,
        }
    }

    /// The debootstrap flavour behind this provider, if it is one.
    pub fn debootstrap(&self) -> Option<Debootstrap> {
        match self {
            Self::Debian => Some(Debootstrap::DEBIAN),
            Self::Ubuntu => Some(Debootstrap::UBUNTU),
            Self::Gentoo | Self::Alpine | Self::Fedora | Self::Buildroot | Self::None => None,
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

/// Alpine's name for a Gentoo-style arch string.
///
/// riscv64 became a supported Alpine architecture in **3.20** (2024-05-22);
/// `ALPINE_BRANCH` older than v3.20 has no riscv64 repository and
/// `alpine_branch_serves()` refuses it rather than letting apk 404.
///
/// No mapping for i586: Alpine's x86 port is compiled with SSE (its
/// busybox alone uses xmm registers), so the pentium-mmx board cannot
/// run it.  i686 maps to `x86` and gets the same SSE floor -- a genuine
/// SSE-less i686 is out too, which the ISA check catches against the
/// assembled image.
pub fn alpine_arch(arch: &str) -> Option<&'static str> {
    match arch {
        "riscv64" => Some("riscv64"),
        "aarch64" => Some("aarch64"),
        "x86_64" => Some("x86_64"),
        "i686" => Some("x86"),
        "armv7a" | "armv7" => Some("armv7"),
        _ => None,
    }
}

/// Whether an `ALPINE_BRANCH` carries a repository for `alpine_arch`.
/// Only says no when it is certain: a `vN.M` branch strictly older than
/// the release that introduced the port.  `edge`, `latest-stable` and
/// anything unparseable pass through.
pub fn alpine_branch_serves(branch: &str, alpine_arch: &str) -> bool {
    let since = match alpine_arch {
        "riscv64" => (3, 20),
        _ => return true,
    };
    let Some((major, minor)) = parse_alpine_branch(branch) else {
        return true;
    };
    (major, minor) >= since
}

/// "v3.24" -> (3, 24).  None for edge, latest-stable, anything else.
fn parse_alpine_branch(branch: &str) -> Option<(u32, u32)> {
    let (major, minor) = branch.strip_prefix('v')?.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

// apk-tools is not in ::gentoo, so the crossdev-stages overlay
// carries the ebuild and the `deps` step emerges it like any other host
// dependency.  Nothing is pinned here: the release tarball and its
// checksums live in the ebuild and its Manifest, where portage enforces
// them and the VDB records what was built.
//
// The Alpine signing keys are a separate matter and stay committed under
// `defaults/alpine-keys/`.  They are the trust anchor, and downloading an
// anchor over the channel it is about to authenticate buys nothing.  Every
// key there was taken from `alpine-keys-2.6-r0.apk` and byte-compared
// against the aports `v3.24.1` tag.  `--allow-untrusted` is never passed.

/// Fedora's name for a Gentoo-style arch string.
///
/// Three architectures, and only three.  Fedora retired ARMv7 in 37
/// (36 is the last release with an `armhfp` tree, EOL since 2023) and
/// has published no 32-bit x86 tree since 25; what is still built i686
/// is multilib *libraries* only, with no i686 `bash`, `coreutils`,
/// `rpm` or `filesystem`, so there is nothing to unpack even before
/// i586's missing SSE2 comes up.
///
/// riscv64 maps, but it is not a Fedora release architecture: see
/// `FedoraImage::url`.
pub fn fedora_arch(arch: &str) -> Option<&'static str> {
    match arch {
        "riscv64" => Some("riscv64"),
        "aarch64" => Some("aarch64"),
        "x86_64" => Some("x86_64"),
        _ => None,
    }
}

/// One pinned Fedora container base image.
///
/// The compose id is part of the file name and moves independently of
/// the release, so both are pinned, and the sha256 with them.  Adding a
/// release means adding rows here, deliberately: a board cannot ask for
/// an image nobody checked.
pub struct FedoraImage {
    pub release: &'static str,
    pub arch: &'static str,
    /// Compose id in the file name.  `1.7` on the primary release
    /// composes, a date stamp on the RISC-V SIG's.
    pub compose: &'static str,
    pub sha256: &'static str,
}

/// Release pinned when a board does not say.
pub const FEDORA_RELEASE_DEFAULT: &str = "44";

/// The images this tree has checksums for.
///
/// aarch64 and x86_64 come from the primary release compose and their
/// sha256 is also published in a clearsigned `Fedora-Container-<rel>-
/// <compose>-<arch>-CHECKSUM`, signed by the Fedora <rel> release key.
/// riscv64's comes from a plain `.sha256` sidecar, because the RISC-V
/// SIG's composes are not signed by anything; that is the whole reason
/// these values are committed here rather than read from the mirror.
pub const FEDORA_IMAGES: &[FedoraImage] = &[
    FedoraImage {
        release: "44",
        arch: "aarch64",
        compose: "1.7",
        sha256: "eca19542a48a8e39b84e869713a1fa2408cbcc578de26c25ae72e3334ef968c1",
    },
    FedoraImage {
        release: "44",
        arch: "riscv64",
        compose: "20260604.0",
        sha256: "198c75fe6f58fea77e539fd29a3103407c0923833c7192c5e962a008c0595f31",
    },
    FedoraImage {
        release: "44",
        arch: "x86_64",
        compose: "1.7",
        sha256: "75200f5752a74a21a616ca9a75e25beb594e2e117a0195c54f87c0b3e3974d1b",
    },
];

/// The pinned image for a release and Fedora arch, if there is one.
pub fn fedora_image(release: &str, arch: &str) -> Option<&'static FedoraImage> {
    FEDORA_IMAGES
        .iter()
        .find(|i| i.release == release && i.arch == arch)
}

/// `"44 (aarch64 riscv64 x86_64)"`, for the error when a board asks for
/// something not in the table.
pub fn fedora_pinned() -> String {
    let mut releases: Vec<&str> = FEDORA_IMAGES.iter().map(|i| i.release).collect();
    releases.dedup();
    releases
        .iter()
        .map(|r| {
            let arches: Vec<&str> = FEDORA_IMAGES
                .iter()
                .filter(|i| i.release == *r)
                .map(|i| i.arch)
                .collect();
            format!("{r} ({})", arches.join(" "))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

impl FedoraImage {
    pub fn file_name(&self) -> String {
        format!(
            "Fedora-Container-Base-Generic-{}-{}.{}.oci.tar.xz",
            self.release, self.compose, self.arch
        )
    }

    /// Two trees, because riscv64 is not a Fedora architecture.  It has
    /// no `releases/<rel>/Container/riscv64/` and nothing under
    /// `fedora-secondary` either (that is ppc64le and s390x); the
    /// RISC-V SIG composes separately and publishes under `/alt`.
    pub fn url(&self, mirror: &str) -> String {
        let mirror = mirror.trim_end_matches('/');
        let dir = if self.arch == "riscv64" {
            format!("alt/risc-v/release/{}/Container/riscv64/images", self.release)
        } else {
            format!(
                "fedora/linux/releases/{}/Container/{}/images",
                self.release, self.arch
            )
        };
        format!("{mirror}/{dir}/{}", self.file_name())
    }
}

#[cfg(test)]
mod tests {
    use super::{Debootstrap, RootfsProvider, SecondStage};

    #[test]
    fn only_alpine_and_fedora_need_the_overlay() {
        assert!(RootfsProvider::Alpine.needs_overlay());
        assert!(RootfsProvider::Fedora.needs_overlay());
        for p in [
            RootfsProvider::Gentoo,
            RootfsProvider::Debian,
            RootfsProvider::Ubuntu,
            RootfsProvider::Buildroot,
            RootfsProvider::None,
        ] {
            assert!(
                !p.needs_overlay(),
                "{} must build without the overlay",
                p.name()
            );
        }
    }

    #[test]
    fn parse_known_values() {
        assert_eq!(RootfsProvider::parse("gentoo"), Some(RootfsProvider::Gentoo));
        assert_eq!(RootfsProvider::parse("debian"), Some(RootfsProvider::Debian));
        assert_eq!(RootfsProvider::parse("ubuntu"), Some(RootfsProvider::Ubuntu));
        assert_eq!(RootfsProvider::parse("alpine"), Some(RootfsProvider::Alpine));
        assert_eq!(RootfsProvider::parse("fedora"), Some(RootfsProvider::Fedora));
        assert_eq!(
            RootfsProvider::parse("buildroot"),
            Some(RootfsProvider::Buildroot)
        );
        assert_eq!(RootfsProvider::parse("none"), Some(RootfsProvider::None));
        assert_eq!(RootfsProvider::parse("suse"), Option::None);
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
            RootfsProvider::Alpine,
            RootfsProvider::Fedora,
            RootfsProvider::Buildroot,
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

    /// Which of (a) and (c) a buildroot board gets is decided by
    /// BUILD_STEPS, not by the provider: no crossdev for a board whose
    /// kernel comes out of the defconfig, crossdev for one that still
    /// builds its own.
    #[test]
    fn buildroot_needs_a_toolchain_only_when_the_board_builds_one_itself() {
        let p = RootfsProvider::Buildroot;
        assert!(!p.needs_cross_toolchain(&["deps", "assemble", "pack"]));
        assert!(p.needs_cross_toolchain(&["deps", "checkout", "kernel", "assemble", "pack"]));
    }

    #[test]
    fn no_provider_but_gentoo_seeds_a_stage3() {
        assert!(RootfsProvider::Gentoo.provisions_stage3());
        assert!(!RootfsProvider::Buildroot.provisions_stage3());
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

    #[test]
    fn alpine_arch_mapping() {
        assert_eq!(super::alpine_arch("riscv64"), Some("riscv64"));
        assert_eq!(super::alpine_arch("aarch64"), Some("aarch64"));
        assert_eq!(super::alpine_arch("x86_64"), Some("x86_64"));
        assert_eq!(super::alpine_arch("i686"), Some("x86"));
        assert_eq!(super::alpine_arch("armv7a"), Some("armv7"));
        // pentium-mmx: Alpine's x86 port needs SSE.
        assert_eq!(super::alpine_arch("i586"), Option::None);
        assert_eq!(super::alpine_arch("riscv32"), Option::None);
    }

    #[test]
    fn riscv64_needs_alpine_320() {
        assert!(!super::alpine_branch_serves("v3.19", "riscv64"));
        assert!(!super::alpine_branch_serves("v3.9", "riscv64"));
        assert!(super::alpine_branch_serves("v3.20", "riscv64"));
        assert!(super::alpine_branch_serves("v3.24", "riscv64"));
        // Unparseable and moving branches are not second-guessed.
        assert!(super::alpine_branch_serves("edge", "riscv64"));
        assert!(super::alpine_branch_serves("latest-stable", "riscv64"));
        // Every other arch predates every branch this can name.
        assert!(super::alpine_branch_serves("v3.9", "aarch64"));
    }

    #[test]
    fn fedora_arch_mapping() {
        assert_eq!(super::fedora_arch("riscv64"), Some("riscv64"));
        assert_eq!(super::fedora_arch("aarch64"), Some("aarch64"));
        assert_eq!(super::fedora_arch("x86_64"), Some("x86_64"));
        // Retired in Fedora 37; 36 is the last release with an armhfp tree.
        assert_eq!(super::fedora_arch("armv7a"), Option::None);
        // i686 is multilib libraries only: no bash, no coreutils, no rpm.
        assert_eq!(super::fedora_arch("i686"), Option::None);
        assert_eq!(super::fedora_arch("i586"), Option::None);
    }

    #[test]
    fn every_pinned_image_has_a_sha256() {
        for i in super::FEDORA_IMAGES {
            assert_eq!(i.sha256.len(), 64, "{} {}", i.release, i.arch);
            assert!(super::fedora_arch(i.arch).is_some());
        }
    }

    #[test]
    fn default_release_is_pinned_for_every_arch() {
        for arch in ["aarch64", "riscv64", "x86_64"] {
            assert!(
                super::fedora_image(super::FEDORA_RELEASE_DEFAULT, arch).is_some(),
                "{arch}"
            );
        }
        assert!(super::fedora_image("40", "aarch64").is_none());
    }

    #[test]
    fn riscv64_comes_from_the_alt_tree() {
        let mirror = "https://dl.fedoraproject.org/pub";
        let rv = super::fedora_image("44", "riscv64").unwrap();
        let aa = super::fedora_image("44", "aarch64").unwrap();
        // riscv64 is not a Fedora release architecture, so it is not
        // under releases/ and not under fedora-secondary/ either.
        assert!(rv.url(mirror).contains("/alt/risc-v/release/44/"));
        assert!(aa.url(mirror).contains("/fedora/linux/releases/44/"));
        assert!(rv.url(mirror).ends_with(&rv.file_name()));
        // A trailing slash on FEDORA_MIRROR must not double up.
        assert_eq!(rv.url(mirror), rv.url("https://dl.fedoraproject.org/pub/"));
    }
}
