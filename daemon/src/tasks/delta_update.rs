use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use log::{debug, error, info, warn};

use crate::{
    app_state::AppState,
    onedrive_service::onedrive_models::DriveItem,
    persistency::{download_queue_repository::DownloadQueueRepository, drive_item_with_fuse_repository::DriveItemWithFuseRepository, processing_item_repository::{ProcessingItem, ProcessingItemRepository}, sync_state_repository::SyncStateRepository},
    scheduler::{PeriodicTask, TaskMetrics},
};

use onedrive_sync_lib::notifications::{NotificationSender, NotificationUrgency};

/// Default sync interval in seconds
const DEFAULT_SYNC_INTERVAL_SECS: u64 = 30; 

/// Default metrics configuration
const DEFAULT_METRICS_WINDOW: usize = 5;
const DEFAULT_SLOW_THRESHOLD_SECS: u64 = 1;

/// OneDrive API path prefix to strip
const DRIVE_ROOT_PREFIX: &str = "/drive/root:/";

/// Change types for delta synchronization
#[derive(Debug, Clone, PartialEq)]
pub enum ChangeType {
    /// New item created
    Create,
    /// Existing item updated
    Update,
    /// Item deleted
    Delete,
    /// Item moved to different location
    Move,
    /// No change detected
    NoChange,
}

/// OneDrive delta synchronization cycle
pub struct SyncCycle {
    app_state: Arc<AppState>,
    processing_repo: ProcessingItemRepository,
    drive_item_with_fuse_repo: DriveItemWithFuseRepository,
}

impl SyncCycle {
    /// Create a new sync cycle
    pub fn new(app_state: Arc<AppState>) -> Self {
        let processing_repo = app_state.persistency().processing_item_repository() ;
        let drive_item_with_fuse_repo = app_state.persistency().drive_item_with_fuse_repository() ;
        Self { app_state, 
        processing_repo, 
        drive_item_with_fuse_repo
        }
    }

    /// Create a periodic task for this sync cycle
    pub async fn get_task(&self) -> Result<PeriodicTask> {
        let metrics = TaskMetrics::new(
            DEFAULT_METRICS_WINDOW,
            Duration::from_secs(DEFAULT_SLOW_THRESHOLD_SECS),
        );

        let app_state = self.app_state.clone();

        let task = PeriodicTask {
            name: "adaptive_sync".to_string(),
            interval: Duration::from_secs(DEFAULT_SYNC_INTERVAL_SECS),
            metrics,
            task: Box::new(move || {
                let app_state = app_state.clone();
                Box::pin(async move {
                                    // Execute sync cycle with panic recovery
                let sync_cycle = SyncCycle::new(app_state);
                let result = sync_cycle.run().await;
                
                if let Err(ref e) = result {
                    error!("Sync cycle failed: {}", e);
                }
                
                result
                })
            }),
        };

        Ok(task)
    }

    /// Retrieve delta changes from OneDrive API with pagination handling
    pub async fn get_delta_changes(&self) -> Result<Vec<DriveItem>> {
        let sync_state_repo = SyncStateRepository::new(self.app_state.persistency().pool().clone());

        let sync_state = sync_state_repo.get_latest_sync_state().await?;
        let delta_token = sync_state
            .map(|(delta_link, _, _)| {
                debug!("🔗 Retrieved delta link from DB: {}", delta_link);
                // Extract token from delta_link URL
                if delta_link.starts_with("http") {
                    // Extract just the token part from the full URL
                    if let Some(token_start) = delta_link.find("token=") {
                        let token = &delta_link[token_start + 6..];
                        debug!("🔑 Extracted token: {}", token);
                        Some(token.to_string())
                    } else {
                        warn!("⚠️ Could not extract token from delta link: {}", delta_link);
                        None
                    }
                } else {
                    // Already just a token
                    debug!("🔑 Using token directly: {}", delta_link);
                    Some(delta_link)
                }
            })
            .unwrap_or(None);

        let mut all_items = Vec::new();
        let mut current_token = delta_token;

        info!("🔄 Starting delta sync with token: {:?}", current_token);

        // Handle pagination and token expiration
        loop {
            match self
                .app_state
                .onedrive_client
                .get_delta_changes(current_token.as_deref())
                .await
            {
                Ok(delta) => {
                    debug!("📥 Received {} items from delta API", delta.value.len());
                    all_items.extend(delta.value);
                    info!("📊 Total delta items count: {}", all_items.len());

                    if let Some(next_link) = delta.next_link {
                        // Continue pagination
                        debug!("⏭️ Continuing pagination with next_link: {}", next_link);
                        current_token = Some(next_link);
                        continue;
                    } else {
                        // Pagination complete, store delta_link for next cycle
                        if let Some(delta_link) = delta.delta_link {
                            debug!("💾 Storing delta link for next cycle: {}", delta_link);
                            sync_state_repo
                                .store_sync_state(Some(delta_link), "done", None)
                                .await
                                .context("Failed to store sync state")?;
                        } else {
                            warn!("⚠️ No delta_link received from API");
                        }
                        break;
                    }
                }

                Err(e) if e.to_string().contains("410") => {
                    // Token expired, restart delta sync
                    warn!("🔄 Delta token expired, restarting sync");
                    current_token = None;
                    continue;
                }

                Err(e) => return Err(e.context("Failed to get delta changes")),
            }
        }
   


        Ok(all_items)
    }

    /// Detect change type based on OneDrive delta response and existing DB state


    /// Check if parent ID changed (indicates move)
    fn parent_id_changed(&self, existing: &DriveItem, new: &DriveItem) -> bool {
        existing.parent_reference.as_ref().map(|p| &p.id)
            != new.parent_reference.as_ref().map(|p| &p.id)
    }

    /// Check if etag changed (indicates file modification)
    fn some_attribute_changed(&self, existing: &DriveItem, new: &DriveItem) -> bool {
        
        let etag_changed = existing.etag != new.etag;
        let name_changed = existing.name != new.name;
        let size_changed = existing.size != new.size;
        let last_modified_changed = existing.last_modified != new.last_modified;
        let created_date_changed = existing.created_date != new.created_date;
        
        if etag_changed || name_changed || size_changed || last_modified_changed || created_date_changed {
            info!("🔄 Item changed: {} (etag: {}, name: {}, size: {}, last_modified: {}, created_date: {})", 
                   new.name.as_deref().unwrap_or("unnamed"),
                   etag_changed, name_changed, size_changed, last_modified_changed, created_date_changed);
            return true;
        } else {
            debug!("✅ Item unchanged: {} (etag matches)", new.name.as_deref().unwrap_or("unnamed"));
            return false;
        }
    }



    

    /// Process download queue
    async fn process_download_queue(&self) -> Result<()> {
        let download_queue_repo =
            DownloadQueueRepository::new(self.app_state.persistency().pool().clone());
        let pending_downloads = download_queue_repo.get_pending_downloads().await?;

        info!(
            "📋 Processing {} pending downloads",
            pending_downloads.len()
        );

        for (queue_id, drive_item_id, local_path) in pending_downloads {
            match self.download_file(&drive_item_id, &local_path).await {
                Ok(_) => {
                    download_queue_repo
                        .mark_download_completed(queue_id)
                        .await?;
                    info!("✅ Download completed: {}", drive_item_id);

                    let drive_item_with_fuse_repo = DriveItemWithFuseRepository::new(self.app_state.persistency().pool().clone());
                    let name = drive_item_with_fuse_repo.get_drive_item_with_fuse(&drive_item_id).await?.unwrap().drive_item.name.unwrap_or("unnamed".to_string());

                    
                    let notification_sender = NotificationSender::new().await;
                    if let Ok(sender) = notification_sender {
                        let filename = name;
                        let _ = sender.send_notification(
                            "Open OneDrive",
                            0,
                            "open-onedrive", 
                            "Open OneDrive",
                            &format!("File {} finished downloading", filename),
                            vec![],
                            vec![("urgency", &NotificationUrgency::Normal.to_u8().to_string())],
                            5000,
                        ).await;
                    }
                    // Get the inode for this file
                    let drive_item_with_fuse_repo = DriveItemWithFuseRepository::new(self.app_state.persistency().pool().clone());
                    if let Ok(Some(item)) = drive_item_with_fuse_repo.get_drive_item_with_fuse(&drive_item_id).await {
                        if let Some(ino) = item.virtual_ino() {
                            // Move the file to the local folder using inode
                            self.app_state.file_manager.move_downloaded_file_to_local_folder(ino).await?;
                        }
                    }
                    
                }
                Err(e) => {
                    download_queue_repo
                        .mark_download_failed(queue_id, 0)
                        .await?;
                    error!("❌ Download failed for {}: {}", drive_item_id, e);
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
            .await
            .context("Failed to get item by ID")?;

        if let Some(download_url) = full_item.download_url {
            // Download file using OneDrive API
            let filename = full_item.name.as_deref().unwrap_or("unnamed");
            let download_result = self
                .app_state
                .onedrive_client
                .download_file(&download_url, drive_item_id, filename)
                .await
                .context("Failed to download file")?;

            // Get the length before moving the data
            let data_len = download_result.file_data.len();

            // Get the inode for this file to determine local path
            let drive_item_with_fuse_repo = DriveItemWithFuseRepository::new(self.app_state.persistency().pool().clone());
            let actual_local_path = if let Ok(Some(item)) = drive_item_with_fuse_repo.get_drive_item_with_fuse(drive_item_id).await {
                if let Some(ino) = item.virtual_ino() {
                    self.app_state.config().project_dirs.data_dir().join("downloads").join(ino.to_string())
                } else {
                    local_path.to_path_buf()
                }
            } else {
                local_path.to_path_buf()
            };

            // Write downloaded data to local file
            std::fs::write(&actual_local_path, download_result.file_data).with_context(|| {
                format!(
                    "Failed to write file {}: {}",
                    actual_local_path.display(),
                    drive_item_id
                )
            })?;

            debug!(
                "📥 Downloaded file: {} -> {} ({} bytes)",
                drive_item_id,
                local_path.display(),
                data_len
            );
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "No download URL available for {}",
                drive_item_id
            ))
        }
    }

    /// Run the complete sync cycle
    pub async fn run(&self) -> Result<()> {

        info!("🔄 Starting two-way sync cycle");

        // Get delta changes from OneDrive
        let items = self.get_delta_changes().await?;
        info!("📊 Retrieved {} delta items", items.len());
        
        // Create ProcessingItems for remote changes (skip items with no actual changes)
        
        for item in &items {
            let change_operation = self.detect_change_operation(item);
            
            // Skip creating processing items for items that haven't actually changed
            if change_operation == crate::persistency::processing_item_repository::ChangeOperation::NoChange {
                debug!("⏭️ Skipping item with no changes: {} ({})", 
                       item.name.as_deref().unwrap_or("unnamed"), item.id);
                continue;
            }
            
            let processing_item = ProcessingItem::new_remote(item.clone(), change_operation);
            let _id = self.processing_repo.store_processing_item(&processing_item).await?;
        }

        // Process all items using the new two-way sync system
        let sync_processor = crate::sync::sync_processor::SyncProcessor::new(self.app_state.clone());
        sync_processor.process_all_items().await?;
        self.process_download_queue().await?;
        //self.process_upload_queue().await?;

        info!("✅ Two-way sync cycle completed");
        Ok(())
    }

    /// Detect change operation based on OneDrive delta response and existing DB state
    fn detect_change_operation(&self, item: &DriveItem) -> crate::persistency::processing_item_repository::ChangeOperation {
       
        
        // Get existing item from DB
        let existing_item = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(
                self.drive_item_with_fuse_repo.get_drive_item_with_fuse(&item.id)
            )
        });

        match existing_item {
            Ok(Some(existing)) => {
                if item.deleted.is_some() {
                    crate::persistency::processing_item_repository::ChangeOperation::Delete
                } else if self.parent_id_changed(&existing.drive_item, item) {
                    crate::persistency::processing_item_repository::ChangeOperation::Move {
                        old_path: existing.virtual_path().unwrap_or_default().to_string(),
                        new_path: item.parent_reference.as_ref().and_then(|p| p.path.clone()).unwrap_or_default(),
                    }
                } else if self.some_attribute_changed(&existing.drive_item, item) {
                    crate::persistency::processing_item_repository::ChangeOperation::Update
                } else {
                    // No attributes changed, including etag - this means no actual change occurred
                    crate::persistency::processing_item_repository::ChangeOperation::NoChange
                }
            }
            Ok(None) => {
                if item.deleted.is_some() {
                    crate::persistency::processing_item_repository::ChangeOperation::Delete
                } else {
                    crate::persistency::processing_item_repository::ChangeOperation::Create
                }
            }
            Err(_) => {
                // If we can't determine, assume it's a create
                crate::persistency::processing_item_repository::ChangeOperation::Create
            }
        }
    }


}
