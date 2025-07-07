//! Cache manager for OneDrive FUSE filesystem

use crate::file_manager::FileManager;
use crate::onedrive_service::onedrive_models::DriveItem;
use crate::operations::path_utils::{virtual_path_to_cache_path, cache_path_to_virtual_path};
use log::{debug, error};
use serde_json;
use std::fs;
use std::path::{Path, PathBuf};

/// Cache manager for OneDrive filesystem operations
pub struct CacheManager {
    file_manager: Box<dyn FileManager>,
}

impl CacheManager {
    pub fn new(file_manager: Box<dyn FileManager>) -> Self {
        Self { file_manager }
    }
    
    /// Get cache path for a virtual path
    pub fn virtual_path_to_cache_path(&self, virtual_path: &Path) -> PathBuf {
        virtual_path_to_cache_path(virtual_path, &self.file_manager.get_cache_dir())
    }
    
    /// Get virtual path from cache path
    pub fn cache_path_to_virtual_path(&self, cache_path: &Path) -> PathBuf {
        cache_path_to_virtual_path(cache_path, &self.file_manager.get_cache_dir())
    }
    
    /// Get temp download path for a virtual path
    pub fn virtual_path_to_temp_path(&self, virtual_path: &Path) -> PathBuf {
        self.file_manager.virtual_path_to_downloaded_path(virtual_path)
    }
    
    /// Read DriveItem from a cache file
    pub fn read_drive_item_from_cache(&self, cache_path: &Path) -> Option<DriveItem> {
        match fs::read_to_string(cache_path) {
            Ok(content) => match serde_json::from_str::<DriveItem>(&content) {
                Ok(item) => Some(item),
                Err(e) => {
                    error!(
                        "Failed to parse DriveItem from {}: {}",
                        cache_path.display(),
                        e
                    );
                    None
                }
            },
            Err(e) => {
                debug!("Failed to read cache file {}: {}", cache_path.display(), e);
                None
            }
        }
    }
    
    /// Check if a file exists in the temp download directory
    pub fn file_exists_in_temp(&self, virtual_path: &Path) -> bool {
        let temp_path = self.virtual_path_to_temp_path(virtual_path);
        temp_path.exists() && temp_path.is_file()
    }
    
    /// Get the temp download path for a virtual path
    pub fn get_temp_path_for_virtual_path(&self, virtual_path: &Path) -> PathBuf {
        self.virtual_path_to_temp_path(virtual_path)
    }
    
    /// Read directory entries from cache
    pub fn read_directory_from_cache(&self, virtual_path: &Path) -> Vec<(String, u64, fuser::FileType)> {
        let cache_path = self.virtual_path_to_cache_path(virtual_path);
        let mut entries = Vec::new();

        if !cache_path.is_dir() {
            return entries;
        }

        match fs::read_dir(&cache_path) {
            Ok(dir_entries) => {
                for entry in dir_entries {
                    if let Ok(entry) = entry {
                        let file_name = entry.file_name().to_string_lossy().to_string();

                        // Skip .dir.json files - they're metadata, not actual entries
                        if file_name == ".dir.json" {
                            continue;
                        }

                        // Construct virtual path for this entry
                        let child_virtual_path = if virtual_path == Path::new("/") {
                            PathBuf::from("/").join(&file_name)
                        } else {
                            virtual_path.join(&file_name)
                        };

                        // Get inode for this entry
                        let child_cache_path = self.virtual_path_to_cache_path(&child_virtual_path);
                        let inode = crate::helpers::path_to_inode(&child_cache_path);
                        
                        // Determine file type
                        let file_type = if child_cache_path.is_dir() {
                            fuser::FileType::Directory
                        } else {
                            fuser::FileType::RegularFile
                        };

                        entries.push((file_name, inode, file_type));
                    }
                }
            }
            Err(e) => {
                error!("Failed to read directory {}: {}", cache_path.display(), e);
            }
        }

        entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_manager::DefaultFileManager;
    use tempfile::tempdir;

    async fn create_test_cache_manager() -> CacheManager {
        let temp_dir = tempdir().unwrap();
        unsafe {
            std::env::set_var("HOME", temp_dir.path());
        }
        let file_manager = DefaultFileManager::new().await.unwrap();
        CacheManager::new(Box::new(file_manager))
    }

    #[tokio::test]
    async fn test_virtual_path_to_cache_path() {
        let cache_manager = create_test_cache_manager().await;
        let virtual_path = Path::new("/Documents/test.txt");
        let cache_path = cache_manager.virtual_path_to_cache_path(virtual_path);
        
        assert!(cache_path.to_string_lossy().contains("cache"));
        assert!(cache_path.to_string_lossy().ends_with("Documents/test.txt"));
    }

    #[tokio::test]
    async fn test_file_exists_in_temp() {
        let cache_manager = create_test_cache_manager().await;
        let virtual_path = Path::new("/test.txt");
        
        // Should return false for non-existent file
        assert!(!cache_manager.file_exists_in_temp(virtual_path));
    }

    #[tokio::test]
    async fn test_read_directory_from_cache() {
        let cache_manager = create_test_cache_manager().await;
        let virtual_path = Path::new("/");
        let entries = cache_manager.read_directory_from_cache(virtual_path);
        
        // Should return empty vector for non-existent directory
        assert!(entries.is_empty());
    }
} 