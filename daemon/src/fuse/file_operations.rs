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

    /// Check if file exists locally with upload folder priority
    pub fn file_exists_locally(&self, onedrive_id: &str) -> Option<PathBuf> {
        self.file_manager.get_local_path_if_file_exists(onedrive_id)
    }

    /// Read data from a local staging folder
    pub fn read_local_file(&self, item: &DriveItemWithFuse, offset: u64, size: u32) -> Result<Vec<u8>> {
        let onedrive_id = item.id();
        
        if let Some(local_path) = self.file_exists_locally(onedrive_id) {
            let mut file = std::fs::File::open(&local_path)?;
            file.seek(std::io::SeekFrom::Start(offset))?;
            
            let mut buffer = vec![0u8; size as usize];
            let bytes_read = file.read(&mut buffer)?;
            buffer.truncate(bytes_read);
            
            Ok(buffer)
        } else {
            // Return placeholder content if file doesn't exist locally
            Ok(self.generate_placeholder_content(item))
        }
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