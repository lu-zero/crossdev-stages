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
    /// Nothing is seeded, installed, or configured — board hooks
    /// (`override-deps.sh`, `post-assemble.sh`, …) fill the rootfs.
    None,
}

impl RootfsProvider {
    /// Parse a `ROOTFS_PROVIDER` board.conf value.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "gentoo" => Some(Self::Gentoo),
            "none" => Some(Self::None),
            _ => Option::None,
        }
    }

}

#[cfg(test)]
mod tests {
    use super::RootfsProvider;

    #[test]
    fn parse_known_values() {
        assert_eq!(RootfsProvider::parse("gentoo"), Some(RootfsProvider::Gentoo));
        assert_eq!(RootfsProvider::parse("none"), Some(RootfsProvider::None));
        assert_eq!(RootfsProvider::parse("debian"), Option::None);
    }

    #[test]
    fn default_is_gentoo() {
        assert_eq!(RootfsProvider::default(), RootfsProvider::Gentoo);
    }
}
