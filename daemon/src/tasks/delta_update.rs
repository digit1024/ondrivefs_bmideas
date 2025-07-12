use std::{sync::Arc, time::Duration};

use anyhow::Result;
use log::{info, debug, warn};

use crate::{
    app_state::AppState,
    onedrive_service::onedrive_models::DriveItem,
    persistency::database::{DriveItemRepository, DownloadQueueRepository, SyncStateRepository},
    scheduler::{PeriodicTask, TaskMetrics},
};

#[derive(Debug, Clone, PartialEq)]
pub enum ChangeType {
    Create,
    Update,
    Delete,
    Move,
    NoChange,
}

pub struct SyncCycle {
    app_state: Arc<AppState>,
}

impl SyncCycle {
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self { app_state }
    }

    pub async fn get_task(&self) -> Result<PeriodicTask> {
        let metrics = TaskMetrics::new(5, Duration::from_secs(1));

        // Clone the app_state to avoid lifetime issues
        let app_state = self.app_state.clone();

        let task = PeriodicTask {
            name: "adaptive_sync".to_string(),
            interval: Duration::from_secs(300), // Start with 5 min interval
            metrics,
            task: Box::new(move || {
                let app_state = app_state.clone();
                Box::pin(async move {
                    // Your sync logic here
                    let sync_cycle = SyncCycle::new(app_state);
                    let _ = sync_cycle.run().await;

                    Ok(())
                })
            }),
        };

        Ok(task)
    }

    pub async fn get_delta_changes(&self) -> Result<Vec<DriveItem>> {
        let sync_state_repo =
            SyncStateRepository::new(self.app_state.persistency_manager.pool().clone());
        let sync_state = sync_state_repo.get_latest_sync_state().await?;
        let delta_token = sync_state
            .map(|(_, _, delta_token)| delta_token)
            .unwrap_or(None);

        let mut all_items = Vec::new();
        let mut current_token = delta_token;
        let mut final_delta_link = None;

        // 🔄 Handle pagination AND token expiration
        loop {
            match self
                .app_state
                .onedrive_client
                .get_delta_changes(current_token.as_deref())
                .await
            {
                Ok(delta) => {
                    all_items.extend(delta.value);
                    info!("Delta items count: {}", all_items.len());

                    if let Some(next_link) = delta.next_link {
                        // Continue pagination
                        current_token = Some(next_link);
                        continue;
                    } else {
                        // Pagination complete, store delta_link for next cycle
                        final_delta_link = delta.delta_link;
                        break;
                    }
                }

                Err(e) if e.to_string().contains("410") => {
                    // Token expired, restart delta sync
                    log::warn!("Delta token expired, restarting sync");
                    current_token = None;
                    continue;
                }

                Err(e) => return Err(e),
            }
        }

        // Store the delta_link for next sync cycle
        if let Some(delta_link) = final_delta_link {
            sync_state_repo
                .store_sync_state(Some(delta_link), "syncing", None)
                .await?;
        }

        Ok(all_items)
    }

    /// Detect change type based on OneDrive delta response and existing DB state
    fn detect_change_type(&self, item: &DriveItem, existing_item: Option<&DriveItem>) -> ChangeType {
        match (existing_item, &item.deleted) {
            (None, Some(_)) => ChangeType::Delete, // Already deleted
            (Some(_), Some(_)) => ChangeType::Delete, // Newly deleted
            (None, None) => ChangeType::Create, // New item
            (Some(existing), None) => {
                // Check if moved (parent changed) or updated (etag changed)
                if self.parent_id_changed(existing, item) {
                    ChangeType::Move
                } else if self.etag_changed(existing, item) {
                    ChangeType::Update
                } else {
                    ChangeType::NoChange
                }
            }
        }
    }

    /// Check if parent ID changed (indicates move)
    fn parent_id_changed(&self, existing: &DriveItem, new: &DriveItem) -> bool {
        existing.parent_reference.as_ref().map(|p| &p.id) != 
        new.parent_reference.as_ref().map(|p| &p.id)
    }

    /// Check if etag changed (indicates file modification)
    fn etag_changed(&self, existing: &DriveItem, new: &DriveItem) -> bool {
        existing.etag != new.etag
    }

    /// Check if item should be downloaded based on virtual path matching
    fn should_download(&self, item: &DriveItem, download_folders: &[String]) -> bool {
        if let Some(parent_ref) = &item.parent_reference {
            if let Some(path) = &parent_ref.path {
                // Remove "/drive/root:" prefix to get virtual path
                let virtual_path = path.strip_prefix("/drive/root:/").unwrap_or(path);
                
                // Check if any download folder matches as prefix (exact case matching)
                download_folders.iter().any(|folder| {
                    virtual_path.starts_with(folder)
                })
            } else {
                false // No path info
            }
        } else {
            false // No parent reference
        }
    }

    /// Process a single delta item
    async fn process_delta_item(
        &self,
        item: &DriveItem,
        download_folders: &[String],
    ) -> Result<()> {
        let drive_item_repo = DriveItemRepository::new(self.app_state.persistency_manager.pool().clone());
        let download_queue_repo = DownloadQueueRepository::new(self.app_state.persistency_manager.pool().clone());
        
        // Get existing item from DB
        let existing_item = drive_item_repo.get_drive_item(&item.id).await?;
        
        // Detect change type
        let change_type = self.detect_change_type(item, existing_item.as_ref());
        let local_path = self.app_state.project_config.project_dirs.data_dir().join("downloads");
        if !local_path.exists() {
            std::fs::create_dir_all(&local_path)?;
        }
        
        match change_type {
            ChangeType::Create => {
                // Store new item
                drive_item_repo.store_drive_item(item, None).await?;
                
                // Add to download queue if it matches download folders
                if self.should_download(item, download_folders) {
                    let local_path = local_path.join(item.id.clone());
                    download_queue_repo.add_to_download_queue(&item.id, &local_path).await?;
                    info!("Added new file to download queue: {} ({})", 
                          item.name.as_deref().unwrap_or("unnamed"), item.id);
                }
            }
            
            ChangeType::Update => {
                // Update existing item
                drive_item_repo.store_drive_item(item, None).await?;
                
                // Check if etag changed and file should be downloaded
                if let Some(existing) = existing_item {
                    if self.etag_changed(&existing, item) && self.should_download(item, download_folders) {
                        let local_path = std::path::PathBuf::from(format!("/downloads/{}", item.id));
                        let local_path = local_path.join(item.id.clone());
                        info!("Added modified file to download queue: {} ({})", 
                              item.name.as_deref().unwrap_or("unnamed"), item.id);
                    }
                }
            }
            
            ChangeType::Delete => {
                // Mark as deleted in DB
                drive_item_repo.store_drive_item(item, None).await?;
                
                // TODO: Delete local file if it exists
                // For now, just log the deletion
                info!("File deleted: {} ({})", 
                      item.name.as_deref().unwrap_or("unnamed"), item.id);
            }
            
            ChangeType::Move => {
                // Update parent reference
                drive_item_repo.store_drive_item(item, None).await?;
                
                // TODO: Handle move logic for "download on demand" later
                info!("File moved: {} ({})", 
                      item.name.as_deref().unwrap_or("unnamed"), item.id);
            }
            
            ChangeType::NoChange => {
                // No action needed
                debug!("No change detected for: {} ({})", 
                       item.name.as_deref().unwrap_or("unnamed"), item.id);
            }
        }
        
        Ok(())
    }

    /// Process download queue
    async fn process_download_queue(&self) -> Result<()> {
        let download_queue_repo = DownloadQueueRepository::new(self.app_state.persistency_manager.pool().clone());
        let pending_downloads = download_queue_repo.get_pending_downloads().await?;
        
        info!("Processing {} pending downloads", pending_downloads.len());
        
        for (queue_id, drive_item_id, local_path) in pending_downloads {
            match self.download_file(&drive_item_id, &local_path).await {
                Ok(_) => {
                    download_queue_repo.mark_download_completed(queue_id).await?;
                    info!("Download completed: {}", drive_item_id);
                }
                Err(e) => {
                    download_queue_repo.mark_download_failed(queue_id, 0).await?;
                    warn!("Download failed for {}: {}", drive_item_id, e);
                    // Skip and retry next cycle as per your strategy
                }
            }
        }
        
        Ok(())
    }

    /// Download a single file
    async fn download_file(&self, drive_item_id: &str, local_path: &std::path::Path) -> Result<()> {
        // Fetch full DriveItem by ID to get download URL
        let full_item = self
            .app_state
            .onedrive_client
            .get_item_by_id(drive_item_id)
            .await?;
        
        if let Some(download_url) = full_item.download_url {
            // Download file using OneDrive API
            let filename = full_item.name.as_deref().unwrap_or("unnamed");
            let download_result = self
                .app_state
                .onedrive_client
                .download_file(&download_url, drive_item_id, filename)
                .await?;
            
            // Get the length before moving the data
            let data_len = download_result.file_data.len();
            
            // Write downloaded data to local file
            std::fs::write(local_path, download_result.file_data).map_err(|e| {
                anyhow::anyhow!("Failed to write file {}: {}", local_path.display(), e)
            })?;
            
            info!("Downloaded file: {} -> {} ({} bytes)", 
                  drive_item_id, local_path.display(), data_len);
            Ok(())
        } else {
            Err(anyhow::anyhow!("No download URL available for {}", drive_item_id))
        }
    }

    pub async fn run(&self) -> Result<()> {
        let download_folders = self
            .app_state
            .project_config
            .settings
            .download_folders
            .clone();
        
        info!("Starting sync cycle with download folders: {:?}", download_folders);
        
        // Get delta changes from OneDrive
        let items = self.get_delta_changes().await?;
        info!("Retrieved {} delta items", items.len());
        
        // Process each delta item
        for item in &items {
            self.process_delta_item(item, &download_folders).await?;
        }
        
        // Process download queue
        self.process_download_queue().await?;
        
        info!("Sync cycle completed");
        Ok(())
    }


}
