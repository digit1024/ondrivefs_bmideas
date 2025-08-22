//! Local file operations for FUSE filesystem

use crate::file_manager::DefaultFileManager;
use crate::persistency::types::DriveItemWithFuse;
use anyhow::Result;
use std::path::PathBuf;

/// File operations manager for the FUSE filesystem
pub struct FileOperationsManager {
    file_manager: Arc<DefaultFileManager>,
}

impl FileOperationsManager {
    pub fn new(file_manager: Arc<DefaultFileManager>) -> Self {
        Self { file_manager }
    }

    /// Check if file exists locally by inode
    pub fn file_exists_locally(&self, ino: u64) -> Option<PathBuf> {
        self.file_manager.get_local_path_if_file_exists(ino)
    }
    pub fn is_synchronized(&self, item: &DriveItemWithFuse) -> bool {
        return item.drive_item().id.starts_with("local_");
    }

    /// Generate placeholder content for files that don't exist locally
    pub fn generate_placeholder_content(&self, item: &DriveItemWithFuse) -> Vec<u8> {
        let name = item.name().unwrap_or("unknown");
        let size = item.size();

        let placeholder = format!(
            "This is a placeholder for file: {}\nSize: {} bytes\nThis file is not yet downloaded locally.",
            name, size
        );

        placeholder.into_bytes()
    }
}

use std::io::{Read, Seek};
use std::sync::Arc;
