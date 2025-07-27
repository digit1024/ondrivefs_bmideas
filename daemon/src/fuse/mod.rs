//! FUSE filesystem implementation for OneDrive
//! 
//! This module contains the complete FUSE filesystem implementation
//! organized into focused submodules for better maintainability.

pub mod filesystem;
pub mod file_handles;
pub mod operations;
pub mod drive_item_manager;
pub mod file_operations;
pub mod attributes;
pub mod database;
pub mod utils;

pub use filesystem::OneDriveFuse; 