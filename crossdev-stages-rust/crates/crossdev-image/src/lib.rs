//! Image building functionality for crossdev-stages
//!
//! This crate provides image building capabilities including:
//! - Source repository management
//! - Build process orchestration
//! - Filesystem creation
//! - Image generation

pub mod builder;
pub mod repositories;

pub use builder::ImageBuilder;
pub use repositories::RepositoryManager;
