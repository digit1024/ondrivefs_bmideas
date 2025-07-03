//! Sync utilities for common synchronization operations

use crate::metadata_manager_for_files::MetadataManagerForFiles;
use crate::onedrive_service::onedrive_models::DriveItem;
use crate::operations::file_ops::{create_or_update_item, delete_item, move_item, should_sync_item};
use crate::operations::path_utils::{get_local_meta_cache_path_for_item, get_local_tmp_path_for_item};
use crate::onedrive_service::onedrive_client::OneDriveClient;
use crate::file_manager::FileManager;
use anyhow::{Context, Result};
use log::{error, info, warn};
use std::path::{Path, PathBuf};

/// Sync operation types
#[derive(Debug, Clone)]
pub enum SyncOperation {
    #[allow(dead_code)]
    Create,
    Update,
    Delete,
    Move,
    Skip,
}

/// Sync operation result
#[derive(Debug)]
pub struct SyncResult {
    pub operation: SyncOperation,
    pub item_id: String,
    #[allow(dead_code)]
    pub path: PathBuf,
    pub success: bool,
    pub error: Option<String>,
}

/// Process a single OneDrive item for synchronization
pub async fn process_sync_item(
    item: &DriveItem,
    file_manager: &impl FileManager,
    metadata_manager: &MetadataManagerForFiles,
    onedrive_client: &OneDriveClient,
    settings_sync_folders: &[String],
) -> Result<SyncResult> {
    let cache_dir = file_manager.get_cache_dir();
    let temp_dir = file_manager.get_temp_download_dir();
    
    // Determine local paths
    let local_cache_path = if item.parent_reference.as_ref().unwrap().path.is_none() {
        PathBuf::from("/")
    } else {
        get_local_meta_cache_path_for_item(item, &cache_dir)
    };
    
    let local_temp_path = get_local_tmp_path_for_item(item, &temp_dir);
    
    // Check if item should be synchronized
    if !should_sync_item(&local_cache_path, &cache_dir, settings_sync_folders) {
        return Ok(SyncResult {
            operation: SyncOperation::Skip,
            item_id: item.id.clone(),
            path: local_cache_path,
            success: true,
            error: None,
        });
    }
    
    // Handle deleted items
    if let Some(_deleted_info) = &item.deleted {
        info!("Processing deleted item: {}", item.id);
        let result = delete_item(item, &cache_dir, &temp_dir).await?;
        
        // Clean up metadata
        cleanup_metadata_for_item(item, metadata_manager)?;
        
        return Ok(SyncResult {
            operation: SyncOperation::Delete,
            item_id: item.id.clone(),
            path: result.path,
            success: result.success,
            error: result.error,
        });
    }
    
    // Handle regular items (files and folders)
    if item.folder.is_some() || item.file.is_some() {
        // Handle root folder
        if item.parent_reference.as_ref().unwrap().path.is_none() {
            let dir_path = cache_dir.join(".dir.json");
            let dir_json = serde_json::to_string(item)?;
            std::fs::write(dir_path, dir_json)?;
            
            return Ok(SyncResult {
                operation: SyncOperation::Update,
                item_id: item.id.clone(),
                path: cache_dir,
                success: true,
                error: None,
            });
        }
        
        // Skip .dir.json files
        if item.name.as_ref().unwrap().eq(".dir.json") {
            return Ok(SyncResult {
                operation: SyncOperation::Skip,
                item_id: item.id.clone(),
                path: local_cache_path,
                success: true,
                error: None,
            });
        }
        
        // Check for moves
        if let Some(old_local_path_str) = metadata_manager.get_local_path_for_onedrive_id(&item.id)? {
            let old_local_path = PathBuf::from(&old_local_path_str);
            let old_temp_path = file_manager.virtual_path_to_downloaded_path(
                &file_manager.cache_path_to_virtual_path(&old_local_path)
            );
            
            if old_local_path != local_cache_path {
                info!("Detected move: {} -> {}", old_local_path.display(), local_cache_path.display());
                
                let result = move_item(item, &old_local_path, &old_temp_path, &cache_dir, &temp_dir).await?;
                
                // Update metadata
                update_metadata_for_item(item, &local_cache_path, metadata_manager)?;
                
                return Ok(SyncResult {
                    operation: SyncOperation::Move,
                    item_id: item.id.clone(),
                    path: result.path,
                    success: result.success,
                    error: result.error,
                });
            }
        }
        
        // Create or update item
        let result = create_or_update_item(item, &cache_dir, &temp_dir).await?;
        
        // Update metadata
        update_metadata_for_item(item, &local_cache_path, metadata_manager)?;
        
        // Download file if needed
        if item.file.is_some() {
            download_file_if_needed(item, &local_temp_path, onedrive_client).await?;
        }
        
        return Ok(SyncResult {
            operation: SyncOperation::Update,
            item_id: item.id.clone(),
            path: result.path,
            success: result.success,
            error: result.error,
        });
    }
    
    // Skip items that are neither files nor folders
    Ok(SyncResult {
        operation: SyncOperation::Skip,
        item_id: item.id.clone(),
        path: local_cache_path,
        success: true,
        error: None,
    })
}

/// Update metadata for an item
fn update_metadata_for_item(
    item: &DriveItem,
    local_path: &Path,
    metadata_manager: &MetadataManagerForFiles,
) -> Result<()> {
    metadata_manager.store_onedrive_id_to_local_path(
        &item.id,
        &local_path.display().to_string(),
    )?;
    
    let inode = crate::helpers::path_to_inode(local_path);
    metadata_manager.store_inode_to_local_path(
        inode,
        local_path.display().to_string().as_str(),
    )?;
    
    Ok(())
}

/// Clean up metadata for a deleted item
fn cleanup_metadata_for_item(
    item: &DriveItem,
    metadata_manager: &MetadataManagerForFiles,
) -> Result<()> {
    // Remove OneDrive ID mapping
    if let Err(e) = metadata_manager.remove_onedrive_id_to_local_path(&item.id) {
        error!("Failed to remove OneDrive ID mapping for {}: {}", item.id, e);
    }
    
    // Remove inode mapping if we can get the local path
    if let Ok(Some(local_path_str)) = metadata_manager.get_local_path_for_onedrive_id(&item.id) {
        let local_path = PathBuf::from(&local_path_str);
        let inode = crate::helpers::path_to_inode(&local_path);
        if let Err(e) = metadata_manager.remove_inode_to_local_path(inode) {
            error!("Failed to remove inode mapping for {}: {}", local_path.display(), e);
        }
    }
    
    Ok(())
}

/// Download file if it doesn't exist locally
async fn download_file_if_needed(
    item: &DriveItem,
    local_temp_path: &Path,
    onedrive_client: &OneDriveClient,
) -> Result<()> {
    if local_temp_path.exists() {
        return Ok(());
    }
    
    let download_url = match &item.download_url {
        Some(url) => url,
        None => {
            warn!("No download URL for file: {}", item.id);
            return Ok(());
        }
    };
    
    let file_name = match &item.name {
        Some(name) => name,
        None => {
            warn!("No file name for file: {}", item.id);
            return Ok(());
        }
    };
    
    info!("Downloading file: {}", local_temp_path.display());
    
    let download_result = onedrive_client.download_file(download_url, &item.id, file_name).await?;
    
    // Create parent directory if it doesn't exist
    if let Some(parent) = local_temp_path.parent() {
        std::fs::create_dir_all(parent)
            .context("Failed to create parent directory for downloaded file")?;
    }
    
    // Save the downloaded file
    std::fs::write(local_temp_path, &download_result.file_data)
        .context("Failed to save downloaded file")?;
    
    info!("Successfully downloaded file: {}", local_temp_path.display());
    Ok(())
}
