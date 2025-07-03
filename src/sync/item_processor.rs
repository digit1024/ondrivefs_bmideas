//! Item processor for handling individual OneDrive items

use crate::file_manager::DefaultFileManager;
use crate::metadata_manager_for_files::MetadataManagerForFiles;
use crate::onedrive_service::onedrive_client::OneDriveClient;
use crate::onedrive_service::onedrive_models::DriveItem;
use crate::sync::sync_utils::process_sync_item;
use anyhow::Result;
use log::info;

/// Processor for individual OneDrive items
pub struct ItemProcessor {
    file_manager: DefaultFileManager,
    metadata_manager: &'static MetadataManagerForFiles,
    onedrive_client: OneDriveClient,
}

impl ItemProcessor {
    /// Create a new item processor
    pub fn new(
        file_manager: DefaultFileManager,
        metadata_manager: &'static MetadataManagerForFiles,
        onedrive_client: OneDriveClient,
    ) -> Self {
        Self {
            file_manager,
            metadata_manager,
            onedrive_client,
        }
    }

    /// Process a single OneDrive item
    pub async fn process_item(
        &self,
        item: &DriveItem,
        sync_folders: &[String],
    ) -> Result<()> {
        info!("Processing item: {} ({:?})", item.id, item.name);
        
        let result = process_sync_item(
            item,
            &self.file_manager,
            self.metadata_manager,
            &self.onedrive_client,
            sync_folders,
        ).await?;
        
        match result.operation {
            crate::sync::sync_utils::SyncOperation::Create => {
                info!("Created item: {}", result.item_id);
            }
            crate::sync::sync_utils::SyncOperation::Update => {
                info!("Updated item: {}", result.item_id);
            }
            crate::sync::sync_utils::SyncOperation::Delete => {
                info!("Deleted item: {}", result.item_id);
            }
            crate::sync::sync_utils::SyncOperation::Move => {
                info!("Moved item: {}", result.item_id);
            }
            crate::sync::sync_utils::SyncOperation::Skip => {
                info!("Skipped item: {}", result.item_id);
            }
        }
        
        if !result.success {
            if let Some(error) = result.error {
                log::warn!("Failed to process item {}: {}", result.item_id, error);
            }
        }
        
        Ok(())
    }
}
