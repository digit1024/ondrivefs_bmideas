//! FUSE filesystem implementation for OneDrive
//!
//! This module contains the complete FUSE filesystem implementation
//! organized into focused submodules for better maintainability.

pub mod attributes;
pub mod database;
pub mod drive_item_manager;
pub mod file_handles;
pub mod file_operations;
pub mod filesystem;
pub mod operations;
pub mod utils;

pub use filesystem::OneDriveFuse;
