//! Image building functionality for crossdev-stages
//!
//! This crate provides image building capabilities including:
//! - Source repository management
//! - Build process orchestration
//! - Filesystem creation
//! - Image generation

pub mod repositories;
pub mod builder;

pub use repositories::RepositoryManager;
pub use builder::ImageBuilder;