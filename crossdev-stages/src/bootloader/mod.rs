//! Bootloader component modules.
//!
//! Each module handles clone + build for one bootloader component.
//! Board-specific overrides (override-{step}.sh) bypass these entirely --
//! the override check lives in image.rs, not here.

pub mod opensbi;
pub mod uboot;
