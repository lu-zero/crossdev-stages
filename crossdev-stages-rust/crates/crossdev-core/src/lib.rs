//! Core cross-compilation functionality for crossdev-stages
//!
//! This crate provides the core cross-compilation logic including:
//! - crossdev environment setup
//! - Package management
//! - Stage creation and management

pub mod crossdev;
pub mod packages;

pub use crossdev::CrossdevManager;
pub use packages::PackageManager;