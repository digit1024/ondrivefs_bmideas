//! Download processor for handling OneDrive file downloads

use std::path::{Path, PathBuf};

use crate::file_manager::{DefaultFileManager, FileManager};
use crate::metadata_manager_for_files::MetadataManagerForFiles;
use crate::onedrive_service::onedrive_client::OneDriveClient;
use crate::onedrive_service::onedrive_models::DriveItem;
use anyhow::{Context, Result};
use log::info;

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



    fn get_download_path(&self, item: &DriveItem) -> PathBuf {
        let original_path = item.parent_reference.as_ref().unwrap().path.as_ref().unwrap();
        //remove /drive/root:/
        let original_path = original_path.trim_start_matches("/drive/root:");
        //conver to downloaded path
        let downloaded_path = self.file_manager.virtual_path_to_downloaded_path(PathBuf::from(original_path).as_path());
        downloaded_path.join(item.name.as_ref().unwrap())
    }
    /// Download a single item
    async fn download_item(&self, item: &DriveItem) -> Result<()> {
        let item_name = item.name.as_deref().unwrap_or("unknown");
        info!("Downloading item: {} ({})", item.id, item_name);
        
        // Check if item has a download URL
        let download_url = item.download_url.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No download URL available for item: {}", item.id))?;
        
        // Download the file using OneDrive client
        let download_result = self.onedrive_client
            .download_file(download_url, &item.id, item_name)
            .await
            .context("Failed to download file from OneDrive")?;
        
        // Save the downloaded file using file manager
        let download_path = self.get_download_path(item);
        
        self.file_manager
            .save_downloaded_file_r(&download_result, &download_path)
            .await
            .context("Failed to save downloaded file")?;
        
        // Store metadata mapping
        self.metadata_manager
            .store_onedrive_id_to_local_path(&item.id, download_path.to_str().unwrap())
            .context("Failed to store metadata mapping")?;
        
        // Remove from download queue on success
        self.metadata_manager.remove_download_items_to_process(&item.id)?;
        
        info!("Successfully downloaded and saved item: {} ({})", item.id, item_name);
        Ok(())
    }
} 