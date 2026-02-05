//! System utilities for crossdev-stages
//!
//! This crate provides system-level utilities including:
//! - Architecture parsing and normalization
//! - Bubblewrap container execution
//! - ldconfig management
//! - File system operations

pub mod arch;
pub mod bubblewrap;
pub mod ldconfig;

pub use arch::{
    arch_to_llvm_target, get_arch_aliases, get_default_arch_for_clap, get_default_flavor,
    parse_arch, Arch,
};
pub use bubblewrap::BubblewrapRunner;
pub use ldconfig::LdconfigManager;
