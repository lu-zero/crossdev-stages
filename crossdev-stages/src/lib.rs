//! crossdev-stages: build Gentoo cross-compilation toolchains inside a
//! hakoniwa sandbox and assemble bootable SBC images.
//!
//! This crate is consumed two ways:
//! - as the `crossdev-stages` binary (see `src/main.rs`), and
//! - as a library, so external tooling/CI can drive sandboxes, cross-emerge
//!   targets, and build images without shelling out to the CLI.

pub mod abi;
pub mod binpkg_meta;
pub mod board;
pub mod bootloader;
pub mod cflags;
pub mod chroot;
pub mod cli;
pub mod container;
pub mod error;
pub mod image;
pub mod isa;
pub mod manifest;
pub mod package_list;
pub mod portage;
pub mod sandbox;
pub mod source_cache;
pub mod stage;
pub mod target;
pub mod workspace;
