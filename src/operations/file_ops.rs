//! File operations for OneDrive synchronization

use crate::onedrive_service::onedrive_models::DriveItem;
use crate::operations::path_utils::{get_local_meta_cache_path_for_item, get_local_tmp_path_for_item};
use crate::operations::retry::{force_remove_dir_all, force_remove_file};
use anyhow::{Context, Result};
use log::{info, warn};
use std::path::{Path, PathBuf};

/// File operation result with metadata
#[derive(Debug)]
pub struct FileOpResult {
    pub success: bool,
    pub path: PathBuf,
    pub operation: String,
    pub error: Option<String>,
}

/// Handle file or directory creation/update
pub async fn create_or_update_item(
    item: &DriveItem,
    cache_dir: &Path,
    temp_dir: &Path,
) -> Result<FileOpResult> {
    let cache_path = get_local_meta_cache_path_for_item(item, cache_dir);
    let temp_path = get_local_tmp_path_for_item(item, temp_dir);
    
    // Create parent directories
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent)
            .context("Failed to create cache parent directory")?;
    }
    
    if let Some(parent) = temp_path.parent() {
        std::fs::create_dir_all(parent)
            .context("Failed to create temp parent directory")?;
    }

    // Serialize item metadata
    let object_json = serde_json::to_string(item)
        .context("Failed to serialize DriveItem to JSON")?;

    let result = if item.folder.is_some() {
        // Handle directory
        std::fs::create_dir_all(&cache_path)
            .context("Failed to create directory")?;
        std::fs::write(cache_path.join(".dir.json"), &object_json)
            .context("Failed to write dir.json")?;
        
        FileOpResult {
            success: true,
            path: cache_path.clone(),
            operation: "create_directory".to_string(),
            error: None,
        }
    } else {
        // Handle file
        std::fs::write(&cache_path, &object_json)
            .context("Failed to write file metadata")?;
        
        FileOpResult {
            success: true,
            path: cache_path.clone(),
            operation: "create_file".to_string(),
            error: None,
        }
    };

    info!("Created/updated item: {} ({})", cache_path.display(), result.operation);
    Ok(result)
}

/// Handle file or directory deletion
pub async fn delete_item(
    item: &DriveItem,
    cache_dir: &Path,
    temp_dir: &Path,
) -> Result<FileOpResult> {
    let cache_path = get_local_meta_cache_path_for_item(item, cache_dir);
    let temp_path = get_local_tmp_path_for_item(item, temp_dir);
    
    let mut errors = Vec::new();
    
    // Delete from cache
    if cache_path.exists() {
        if item.folder.is_some() {
            if let Err(e) = force_remove_dir_all(&cache_path).await {
                errors.push(format!("Failed to remove cache directory {}: {}", cache_path.display(), e));
            }
        } else {
            if let Err(e) = force_remove_file(&cache_path).await {
                errors.push(format!("Failed to remove cache file {}: {}", cache_path.display(), e));
            }
        }
    }
    
    // Delete from temp
    if temp_path.exists() {
        if item.folder.is_some() {
            if let Err(e) = force_remove_dir_all(&temp_path).await {
                errors.push(format!("Failed to remove temp directory {}: {}", temp_path.display(), e));
            }
        } else {
            if let Err(e) = force_remove_file(&temp_path).await {
                errors.push(format!("Failed to remove temp file {}: {}", temp_path.display(), e));
            }
        }
    }
    
    let success = errors.is_empty();
    let error = if success { None } else { Some(errors.join("; ")) };
    
    let result = FileOpResult {
        success,
        path: cache_path.clone(),
        operation: "delete_item".to_string(),
        error: error.clone(),
    };
    
    if success {
        info!("Successfully deleted item: {}", cache_path.display());
    } else {
        warn!("Failed to delete item {}: {}", cache_path.display(), error.as_ref().unwrap());
    }
    
    Ok(result)
}

/// Handle item move (delete from old location, create at new location)
pub async fn move_item(
    item: &DriveItem,
    old_cache_path: &Path,
    old_temp_path: &Path,
    cache_dir: &Path,
    temp_dir: &Path,
) -> Result<FileOpResult> {
    info!(
        "Moving item: {} -> {}",
        old_cache_path.display(),
        get_local_meta_cache_path_for_item(item, cache_dir).display()
    );
    
    // Delete from old location
    if old_cache_path.exists() {
        if item.folder.is_some() {
            force_remove_dir_all(old_cache_path).await
                .context("Failed to remove old cache directory")?;
        } else {
            force_remove_file(old_cache_path).await
                .context("Failed to remove old cache file")?;
        }
    }
    
    if old_temp_path.exists() {
        if item.folder.is_some() {
            force_remove_dir_all(old_temp_path).await
                .context("Failed to remove old temp directory")?;
        } else {
            force_remove_file(old_temp_path).await
                .context("Failed to remove old temp file")?;
        }
    }
    
    // Create at new location
    create_or_update_item(item, cache_dir, temp_dir).await
}

/// Save downloaded file content
#[allow(dead_code)]
pub async fn save_downloaded_file(
    file_data: &[u8],
    target_path: &Path,
) -> Result<FileOpResult> {
    // Create parent directory if it doesn't exist
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)
            .context("Failed to create parent directory")?;
    }

    // Write file data
    std::fs::write(target_path, file_data)
        .context("Failed to write file")?;

    info!("Saved downloaded file: {}", target_path.display());
    
    Ok(FileOpResult {
        success: true,
        path: target_path.to_path_buf(),
        operation: "save_downloaded_file".to_string(),
        error: None,
    })
}

/// Check if item should be synchronized based on sync folders configuration
pub fn should_sync_item(
    item_path: &Path,
    cache_dir: &Path,
    sync_folders: &[String],
) -> bool {
    // If no sync folders specified, sync everything
    if sync_folders.is_empty() {
        return true;
    }
    
    // Check if item is in any of the sync folders
    for folder in sync_folders {
        let sync_path = cache_dir.join(folder);
        if item_path.starts_with(&sync_path) {
            return true;
        }
    }
    
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onedrive_service::onedrive_models::{DriveItem, ParentReference};
    use tempfile::tempdir;

    fn create_test_item(name: &str, parent_path: &str, is_folder: bool) -> DriveItem {
        DriveItem {
            id: "test-id".to_string(),
            name: Some(name.to_string()),
            parent_reference: Some(ParentReference {
                path: Some(parent_path.to_string()),
                ..Default::default()
            }),
            folder: if is_folder { Some(Default::default()) } else { None },
            file: if !is_folder { Some(Default::default()) } else { None },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_create_or_update_item_file() {
        let temp_dir = tempdir().unwrap();
        let cache_dir = temp_dir.path().join("cache");
        let temp_download_dir = temp_dir.path().join("temp");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::create_dir_all(&temp_download_dir).unwrap();
        
        let item = create_test_item("test.txt", "/drive/root:/Documents", false);
        let result = create_or_update_item(&item, &cache_dir, &temp_download_dir).await.unwrap();
        
        assert!(result.success);
        assert_eq!(result.operation, "create_file");
        assert!(result.path.exists());
    }

    #[tokio::test]
    async fn test_create_or_update_item_folder() {
        let temp_dir = tempdir().unwrap();
        let cache_dir = temp_dir.path().join("cache");
        let temp_download_dir = temp_dir.path().join("temp");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::create_dir_all(&temp_download_dir).unwrap();
        
        let item = create_test_item("test_folder", "/drive/root:/Documents", true);
        let result = create_or_update_item(&item, &cache_dir, &temp_download_dir).await.unwrap();
        
        assert!(result.success);
        assert_eq!(result.operation, "create_directory");
        assert!(result.path.exists());
        assert!(result.path.join(".dir.json").exists());
    }

    #[test]
    fn test_should_sync_item() {
        let cache_dir = PathBuf::from("/cache");
        let sync_folders = vec!["Documents".to_string(), "Work".to_string()];
        
        // Item in sync folder
        let item_path = PathBuf::from("/cache/Documents/test.txt");
        assert!(should_sync_item(&item_path, &cache_dir, &sync_folders));
        
        // Item not in sync folder
        let item_path = PathBuf::from("/cache/Other/test.txt");
        assert!(!should_sync_item(&item_path, &cache_dir, &sync_folders));
        
        // Empty sync folders - should sync everything
        let item_path = PathBuf::from("/cache/Any/test.txt");
        assert!(should_sync_item(&item_path, &cache_dir, &[]));
    }
} 