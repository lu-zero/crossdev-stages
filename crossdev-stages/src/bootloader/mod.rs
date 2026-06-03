//! Bootloader component modules.
//!
//! Each module handles clone + build for one bootloader component.
//! Board-specific overrides (override-{step}.sh) bypass these entirely --
//! the override check lives in image.rs, not here.

pub mod grub;
pub mod opensbi;
pub mod syslinux;
pub mod uboot;
