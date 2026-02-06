//! Crossdev Stages - Combined cache and stage3 functionality
//!
//! This crate provides both:
//! 1. XDG-compliant caching system for Gentoo packages and distfiles
//! 2. Stage3 image fetching, caching, and extraction for cross-compilation

pub mod cache;
pub mod error;
pub mod stage3;

// Re-export key types for convenience
pub use cache::{CacheConfig, CacheError, CacheStrategy, CrossdevCache};
pub use error::StageError;
pub use stage3::{Stage3Error, Stage3Fetcher, Stage3Info};
