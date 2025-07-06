//! Download processor for handling OneDrive file downloads

use crate::file_manager::DefaultFileManager;
use crate::metadata_manager_for_files::MetadataManagerForFiles;
use crate::onedrive_service::onedrive_client::OneDriveClient;
use crate::onedrive_service::onedrive_models::DriveItem;
use anyhow::{Context, Result};
use log::{info, error};

/// Processor for downloading OneDrive files
#[derive(Clone)]
pub struct DownloadProcessor {
    file_manager: DefaultFileManager,
    metadata_manager: &'static MetadataManagerForFiles,
    onedrive_client: OneDriveClient,
}

impl DownloadProcessor {
    /// Create a new download processor
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

    /// Process all items in the download queue
    pub async fn process_download_queue(&self, sync_folders: &[String]) -> Result<()> {
        let download_items = self.metadata_manager.get_download_items_to_process()?;
        info!("Checking Download Queue. Processing {} download items", download_items.len());
        for item in download_items {
            
            self.download_item(&item).await.context(format!("Error while downloading file {}", item.name.as_deref().unwrap_or("unknown")))?
        }
        Ok(())
    }


    /// Download a single item
    async fn download_item(&self, item: &DriveItem) -> Result<()> {
        info!("Downloading item: {} ({})", item.id, item.name.as_deref().unwrap_or("unknown"));
        
        // TODO: Implement actual download logic
        // For now, just remove from queue to test the flow
        self.metadata_manager.remove_download_items_to_process(&item.id)?;
        info!("Successfully processed download for item: {}", item.id);
        
        Ok(())
    }
} 