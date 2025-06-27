//! SyncService: Handles all OneDrive <-> local sync logic.

use crate::onedrive_service::onedrive_client::OneDriveClient;
use crate::config::{Settings, SyncConfig};
use crate::metadata_manager_for_files::MetadataManagerForFiles;
use crate::file_manager::{FileManager, DefaultFileManager};
use anyhow::Result;
use std::path::{Path, PathBuf};
use tokio::time::sleep;
use tokio::signal;
use log::{info, error};
use log::warn;
use std::str::FromStr;

/// Service responsible for synchronizing files between OneDrive and local storage.
pub struct SyncService {
    pub client: OneDriveClient,
    pub file_manager: DefaultFileManager,
    pub config: SyncConfig,
    pub settings: Settings,
}

impl SyncService {
    pub async fn new(client: OneDriveClient, config: SyncConfig, settings: Settings) -> Result<Self> {
        let metadata_manager = MetadataManagerForFiles::new()?;
        let file_manager = DefaultFileManager::new(metadata_manager).await?;
        
        Ok(Self { 
            client, 
            file_manager,
            config, 
            settings 
        })
    }

    /// Sync files from OneDrive to local directory using delta queries
    pub async fn sync_from_remote(&mut self) -> Result<()> {
        info!("Starting sync from remote using delta queries...");
        
        // Collect folders to avoid borrow checker issues
        let folders: Vec<String> = self.settings.sync_folders.clone();

        for folder in folders {
            info!("Processing folder: {}", folder);
            
            // Step 1: Get changes (delta or initial sync)
            let changes = self.get_folder_changes(&folder).await?;
            
            // Step 2: Process changes in order (maintaining delta order is crucial)
            self.process_delta_changes(&folder, &changes).await?;
            
            // Store delta token for next sync
            if let Some(delta_link) = &changes.delta_link {
                if let Some(token) = Self::extract_delta_token(delta_link) {
                    self.file_manager.metadata_manager().store_folder_delta(&folder, &token)?;
                    info!("Saved delta token for folder: {}", folder);
                }
            }
        }
        
        self.file_manager.metadata_manager().flush()?;
        Ok(())
    }

    /// Step 1: Get folder changes (delta or initial sync)
    async fn get_folder_changes(&self, folder: &str) -> Result<crate::onedrive_service::onedrive_models::DriveItemCollection> {
        let delta_token = self.file_manager.metadata_manager().get_folder_delta(folder)?;
        
        if let Some(delta) = delta_token {
            info!("Delta token found for folder: {}", folder);
            self.client.get_delta_with_token(folder, &delta.delta_token).await
        } else {
            info!("Initial sync for folder: {}", folder);
            // Clean up local directory for initial sync
            self.clean_local_directory_for_initial_sync(folder).await?;
            self.client.get_initial_delta(folder).await
        }
    }

    /// Clean local directory for initial sync
    async fn clean_local_directory_for_initial_sync(&self, folder: &str) -> Result<()> {
        let local_folder = if folder == "/" {
            self.config.local_dir.clone()
        } else {
            let mut p = self.config.local_dir.clone();
            p.push(folder.trim_start_matches('/'));
            p
        };
        
        if local_folder.exists() {
            for entry in std::fs::read_dir(&local_folder)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    std::fs::remove_file(&path)?;
                    info!("Removed local file on initial sync: {:?}", path);
                }
            }
        }
        Ok(())
    }

    /// Step 2: Process delta changes in order
    async fn process_delta_changes(&mut self, folder: &str, changes: &crate::onedrive_service::onedrive_models::DriveItemCollection) -> Result<()> {
        for item in &changes.value {
            // a) Get remote path from parent
            let remote_path = self.get_remote_path_from_item(folder, item)?;
            
            // b) Construct corresponding local path
            let local_path = self.get_local_path_for_item( item);
            
            // c) Check if deleted
            if item.deleted.is_some() {
                self.handle_deleted_item(&item.id, &local_path).await?;
                continue;
            }
            
            // d) Check if etag matches (skip if no change needed)
            if self.should_skip_item(item, &local_path)? {
                info!("Skipping unchanged item: {}", item.name.as_ref().unwrap_or(&"Unknown".to_string()));
                continue;
            }
            
            // e) Perform appropriate action
            if item.folder.is_some() {
                self.handle_folder_item(item, &local_path).await?;
            } else if item.file.is_some() {
                self.handle_file_item(item, &remote_path, &local_path).await?;
            }
            
            // f) Update metadata
            self.update_item_metadata(item, &local_path).await?;
        }
        Ok(())
    }

    /// Get remote path from item's parent reference
    fn get_remote_path_from_item(&self, _folder: &str, item: &crate::onedrive_service::onedrive_models::DriveItem) -> Result<String> {
        let remote_path_from_parent = item.parent_reference.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No parent reference for item"))?
            .path.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No path in parent reference"))?;
        
        // Remove /drive/root: prefix
        let remote_path_from_parent = remote_path_from_parent.trim_start_matches("/drive/root:");
        
        let remote_path = format!("{}/{}", remote_path_from_parent, item.name.as_ref().map_or("Unknown", |v| v));
        Ok(remote_path)
    }

    /// Check if item should be skipped (etag matches)
    fn should_skip_item(&self, item: &crate::onedrive_service::onedrive_models::DriveItem, local_path: &Path) -> Result<bool> {
        if let Some(etag) = &item.etag {
            if let Ok(Some(meta)) = self.file_manager.metadata_manager().get_onedrive_file_meta(&local_path.to_string_lossy()) {
                return Ok(meta.etag == *etag);
            }
        }
        Ok(false)
    }

    /// Handle deleted item
    async fn handle_deleted_item(&self, item_id: &str, local_path: &Path) -> Result<()> {
        if let Some(stored_local_path) = self.file_manager.metadata_manager().get_local_path_from_one_drive_id(item_id)? {
            let stored_path = PathBuf::from_str(&stored_local_path)?;
            if stored_path.exists() {
                if let Err(e) = self.file_manager.delete_file(&stored_path).await {
                    error!("Failed to delete local file {:?}: {}", stored_path, e);
                } else {
                    info!("Deleted local file: {:?}", stored_path);
                }
            }
        }
        // Delete metadata
        self.file_manager.metadata_manager().delete_metadata_for_file(item_id)?;
        Ok(())
    }

    /// Handle folder item (create directory)
    async fn handle_folder_item(&self, item: &crate::onedrive_service::onedrive_models::DriveItem, local_path: &Path) -> Result<()> {
        if let Some(name) = &item.name {
            info!("Creating local directory: {:?}", local_path);
            if !local_path.exists() {
                if let Err(e) = self.file_manager.create_directory(local_path).await {
                    error!("Failed to create directory {:?}: {}", local_path, e);
                } else {
                    info!("Created local directory: {:?}", local_path);
                }
            }
        }
        Ok(())
    }

    /// Handle file item (download file)
    async fn handle_file_item(&self, item: &crate::onedrive_service::onedrive_models::DriveItem, remote_path: &str, local_path: &Path) -> Result<()> {
        let file_name = item.name.as_ref().map_or("Unknown", |v| v);
        
        // Get full item details to get download URL
        match self.client.get_item_by_path(remote_path).await {
            Ok(full_item) => {
                if let Some(download_url) = &full_item.download_url {
                    match self.client.download_file(download_url, &item.id, file_name).await {
                        Ok(download_result) => {
                            match self.file_manager.save_downloaded_file(&download_result, local_path).await {
                                Ok(_) => info!("Downloaded: {} -> {:?}", file_name, local_path),
                                Err(e) => error!("Failed to save downloaded file {}: {}", file_name, e),
                            }
                        },
                        Err(e) => error!("Failed to download {}: {}", file_name, e),
                    }
                } else {
                    error!("No download URL available for file: {}", file_name);
                }
            }
            Err(e) => {
                error!("Failed to get download URL for {}: {}", file_name, e);
            }
        }
        Ok(())
    }

    /// Update metadata for item
    async fn update_item_metadata(&self, item: &crate::onedrive_service::onedrive_models::DriveItem, local_path: &Path) -> Result<()> {
        // Metadata is already updated in save_downloaded_file for files
        // For folders, we need to add metadata
        if item.folder.is_some() {
            self.file_manager.metadata_manager().add_metadata_for_file(&item.id, local_path)?;
        }
        Ok(())
    }

    /// Run a single sync cycle
    pub async fn sync_cycle(&mut self) -> Result<()> {
        info!("Starting sync cycle");
        if let Err(e) = self.sync_from_remote().await {
            error!("Failed to sync from remote: {}", e);
        }
        // TODO: Implement local-to-remote sync
        // if let Err(e) = self.sync_to_remote().await {
        //     error!("Failed to sync to remote: {}", e);
        // }
        info!("Sync cycle completed");
        Ok(())
    }

    /// Helper: Get local path for a OneDrive item
    pub fn get_local_path_for_item(&self, item: &crate::onedrive_service::onedrive_models::DriveItem) -> PathBuf {
 
        // Get the remote path from the parent reference
        let remote_path_from_parent = item.parent_reference.as_ref().unwrap().path.as_ref().unwrap();
        // Remove the /drive/root:/ prefix - for joining the path
        let remote_path_from_parent = remote_path_from_parent.trim_start_matches("/drive/root:/").to_string();
        // Get the synchronization root
        let synchronization_root  : PathBuf = self.config.local_dir.clone();
        // Get the folder path and join it with the item name
        let file_path = synchronization_root.join(remote_path_from_parent).join(item.name.as_ref().unwrap());

        // Return the file path (wow this is nicely encapsulated logic)
        file_path
    }
    
    pub fn get_local_root_path(&self) -> PathBuf {
        let local_path = self.config.local_dir.clone();
        local_path
    }

    pub async fn ensure_authorized(&self) -> Result<()> {
        match self.client.list_root().await {
            Ok(_) => {
                info!("Already authorized and tokens are valid");
                Ok(())
            }
            Err(_) => {
                warn!("Authorization needed or tokens expired");
                let auth = crate::auth::onedrive_auth::OneDriveAuth::new()?;
                auth.get_valid_token().await?;
                info!("Authorization completed");
                Ok(())
            }
        }
    }

    /// Run the sync daemon
    pub async fn run_daemon(&mut self) -> Result<()> {
        info!("Starting OneDrive sync daemon");
        
        // Ensure we're authorized
        self.ensure_authorized().await?;
        
        loop {
            if let Err(e) = self.sync_cycle().await {
                error!("Sync cycle failed: {}", e);
            }
            
            // Wait for next sync cycle or shutdown signal
            tokio::select! {
                _ = sleep(self.config.sync_interval) => {
                    // Continue to next sync cycle
                }
                _ = signal::ctrl_c() => {
                    info!("Received shutdown signal");
                    break;
                }
            }
        }
        
        info!("OneDrive sync daemon stopped");
        Ok(())
    }

    /// Extract delta token from delta link URL
    pub fn extract_delta_token(delta_link: &str) -> Option<String> {
        if let Some(token_start) = delta_link.find("token=") {
            let token = &delta_link[token_start + 6..];
            Some(token.to_string())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    #[tokio::test]
    #[serial]
    async fn test_sync_service_skeleton() {
        // TODO: Setup a mock OneDriveClient, config, and settings
        // let client = ...;
        // let config = ...;
        // let settings = ...;
        // let mut service = SyncService::new(client, config, settings);
        // assert!(service.sync_from_remote().await.is_ok());
        assert!(true); // Placeholder
    }
} 