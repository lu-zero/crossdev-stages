//! Bootloader component modules.
//!
//! Each module handles clone + build for one bootloader component.
//! Board-specific overrides (board.sh) bypass these entirely --
//! the override check lives in image.rs, not here.

pub mod opensbi;
pub mod uboot;

// Future ARM bootloader components:
// pub mod tfa;    // ARM Trusted Firmware-A
// pub mod rkbin;  // Rockchip DDR init blob
