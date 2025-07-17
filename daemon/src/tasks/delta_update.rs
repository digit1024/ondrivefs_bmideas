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
const DEFAULT_SYNC_INTERVAL_SECS: u64 = 30; // 5 minutes

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
}

impl SyncCycle {
    /// Create a new sync cycle
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self { app_state }
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
                    let sync_cycle = SyncCycle::new(app_state);
                    sync_cycle.run().await
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
                info!("🔗 Retrieved delta link from DB: {}", delta_link);
                // Extract token from delta_link URL
                if delta_link.starts_with("http") {
                    // Extract just the token part from the full URL
                    if let Some(token_start) = delta_link.find("token=") {
                        let token = &delta_link[token_start + 6..];
                        info!("🔑 Extracted token: {}", token);
                        Some(token.to_string())
                    } else {
                        warn!("⚠️ Could not extract token from delta link: {}", delta_link);
                        None
                    }
                } else {
                    // Already just a token
                    info!("🔑 Using token directly: {}", delta_link);
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
                    info!("📥 Received {} items from delta API", delta.value.len());
                    all_items.extend(delta.value);
                    info!("📊 Total delta items count: {}", all_items.len());

                    if let Some(next_link) = delta.next_link {
                        // Continue pagination
                        info!("⏭️ Continuing pagination with next_link: {}", next_link);
                        current_token = Some(next_link);
                        continue;
                    } else {
                        // Pagination complete, store delta_link for next cycle
                        if let Some(delta_link) = delta.delta_link {
                            info!("💾 Storing delta link for next cycle: {}", delta_link);
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
    fn detect_change_type(
        &self,
        item: &DriveItem,
        existing_item: Option<&DriveItem>,
    ) -> ChangeType {
        match (existing_item, &item.deleted) {
            (None, Some(_)) => ChangeType::Delete,    // Already deleted
            (Some(_), Some(_)) => ChangeType::Delete, // Newly deleted
            (None, None) => ChangeType::Create,       // New item
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
        existing.parent_reference.as_ref().map(|p| &p.id)
            != new.parent_reference.as_ref().map(|p| &p.id)
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
                let virtual_path = path.strip_prefix(DRIVE_ROOT_PREFIX).unwrap_or(path);

                // Check if any download folder matches as prefix (exact case matching)
                download_folders
                    .iter()
                    .any(|folder| virtual_path.starts_with(folder))
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
        let drive_item_with_fuse_repo = DriveItemWithFuseRepository::new(self.app_state.persistency().pool().clone());
        let download_queue_repo =
            DownloadQueueRepository::new(self.app_state.persistency().pool().clone());

        // Get existing item from DB
        let existing_item = drive_item_with_fuse_repo.get_drive_item_with_fuse(&item.id).await?;

        // Detect change type
        let change_type = self.detect_change_type(item, existing_item.as_ref().map(|i| &i.drive_item));
        let local_path = self
            .app_state
            .config()
            .project_dirs
            .data_dir()
            .join("downloads");

        // Ensure downloads directory exists
        Self::ensure_downloads_directory(&local_path)?;

        match change_type {
            ChangeType::Create => {
                self.handle_create_item(
                    item,
                    &drive_item_with_fuse_repo,
                    &download_queue_repo,
                    &local_path,
                    download_folders,
                )
                .await?;
            }

            ChangeType::Update => {
                self.handle_update_item(
                    item,
                    &drive_item_with_fuse_repo,
                    &download_queue_repo,
                    &local_path,
                    download_folders,
                    existing_item.as_ref(),
                )
                .await?;
            }

            ChangeType::Delete => {
                self.handle_delete_item(item, &drive_item_with_fuse_repo).await?;
            }

            ChangeType::Move => {
                self.handle_move_item(item, &drive_item_with_fuse_repo).await?;
            }

            ChangeType::NoChange => {
                debug!(
                    "⏭️ No change detected for: {} ({})",
                    item.name.as_deref().unwrap_or("unnamed"),
                    item.id
                );
            }
        }

        Ok(())
    }

    /// Set up FUSE metadata for a delta item
    async fn setup_fuse_metadata(
        &self,
        item: &DriveItem,
        drive_item_with_fuse_repo: &DriveItemWithFuseRepository,
        local_path: &std::path::Path,
    ) -> Result<u64> {
        // Check if item already exists to preserve its inode
        let existing_item = drive_item_with_fuse_repo.get_drive_item_with_fuse(&item.id).await?;
        
        // Create the item with basic FUSE metadata
        let mut item_with_fuse = drive_item_with_fuse_repo.create_from_drive_item(item.clone());
        
        // Set file source to Remote since this comes from OneDrive
        item_with_fuse.set_file_source(crate::persistency::types::FileSource::Remote);
        item_with_fuse.set_sync_status("synced".to_string());
        
        // Set local path for downloaded files
        let local_file_path = local_path.join(item.id.clone());
        item_with_fuse.set_display_path(local_file_path.to_string_lossy().to_string());
        
        // Preserve existing inode if item already exists
        if let Some(existing) = &existing_item {
            if let Some(existing_ino) = existing.virtual_ino() {
                item_with_fuse.set_virtual_ino(existing_ino);
            }
        }
        
        // Resolve parent inode if this item has a parent
        if let Some(parent_ref) = &item.parent_reference {
            let parent_id = &parent_ref.id;
            // Get parent item to find its inode
            if let Ok(Some(parent_item)) = drive_item_with_fuse_repo.get_drive_item_with_fuse(parent_id).await {
                if let Some(parent_ino) = parent_item.virtual_ino() {
                    item_with_fuse.set_parent_ino(parent_ino);
                }
            }
        }
        
        // Store the item and get the inode (preserved or new)
        let inode = drive_item_with_fuse_repo.store_drive_item_with_fuse(&item_with_fuse, Some(local_file_path.clone())).await?;
        
        Ok(inode)
    }

    /// Handle creation of new items
    async fn handle_create_item(
        &self,
        item: &DriveItem,
        drive_item_with_fuse_repo: &DriveItemWithFuseRepository,
        download_queue_repo: &DownloadQueueRepository,
        local_path: &std::path::Path,
        download_folders: &[String],
    ) -> Result<()> {
        // Store new item with proper Fuse metadata
        let inode = self.setup_fuse_metadata(item, drive_item_with_fuse_repo, local_path).await?;
        
        info!(
            "📁 Created item: {} ({}) with inode {}",
            item.name.as_deref().unwrap_or("unnamed"),
            item.id,
            inode
        );

        // Add to download queue if it matches download folders
        if self.should_download(item, download_folders) {
            let local_file_path = local_path.join(item.id.clone());
            download_queue_repo
                .add_to_download_queue(&item.id, &local_file_path)
                .await?;
            info!(
                "📥 Added new file to download queue: {} ({})",
                item.name.as_deref().unwrap_or("unnamed"),
                item.id
            );
        }
        Ok(())
    }

    /// Handle updates to existing items
    async fn handle_update_item(
        &self,
        item: &DriveItem,
        drive_item_with_fuse_repo: &DriveItemWithFuseRepository,
        download_queue_repo: &DownloadQueueRepository,
        local_path: &std::path::Path,
        download_folders: &[String],
        existing_item: Option<&crate::persistency::types::DriveItemWithFuse>,
    ) -> Result<()> {
        // Update existing item with proper Fuse metadata
        let inode = self.setup_fuse_metadata(item, drive_item_with_fuse_repo, local_path).await?;
        
        info!(
            "📝 Updated item: {} ({}) with inode {}",
            item.name.as_deref().unwrap_or("unnamed"),
            item.id,
            inode
        );

        // Check if etag changed and file should be downloaded
        if let Some(existing) = existing_item {
            if self.etag_changed(&existing.drive_item, item) && self.should_download(item, download_folders) {
                let local_file_path = local_path.join(item.id.clone());
                download_queue_repo
                    .add_to_download_queue(&item.id, &local_file_path)
                    .await?;
                info!(
                    "📥 Added modified file to download queue: {} ({})",
                    item.name.as_deref().unwrap_or("unnamed"),
                    item.id
                );
            }
        }
        Ok(())
    }

    /// Handle deletion of items
    async fn handle_delete_item(
        &self,
        item: &DriveItem,
        drive_item_with_fuse_repo: &DriveItemWithFuseRepository,
    ) -> Result<()> {
        // Remove from download queue if it's pending
        let download_queue_repo = DownloadQueueRepository::new(self.app_state.persistency().pool().clone());
        let local_path = self
            .app_state
            .config()
            .project_dirs
            .data_dir()
            .join("downloads");
        let local_file_path = local_path.join(item.id.clone());
        
        // Remove item from download queue if it exists
        if let Err(e) = download_queue_repo.remove_by_drive_item_id(&item.id).await {
            warn!("⚠️ Failed to remove item from download queue: {}", e);
        } else {
            info!("📋 Removed deleted item from download queue: {}", item.id);
        }

        // If it's a folder, also remove all child items from download queue and delete their local files
        if item.folder.is_some() {
            self.remove_child_items_from_download_queue(&item.id, &download_queue_repo).await?;
            self.delete_child_local_files(&item.id, &local_path).await?;
        }
        // Mark as deleted in DB with Fuse metadata
        let local_path = self
            .app_state
            .config()
            .project_dirs
            .data_dir()
            .join("downloads");
        let inode = self.setup_fuse_metadata(item, drive_item_with_fuse_repo, &local_path).await?;

        // Delete local file if it exists
        let local_file_path = local_path.join(item.id.clone());
        if local_file_path.exists() {
            match std::fs::remove_file(&local_file_path) {
                Ok(_) => {
                    info!(
                        "🗑️ Deleted local file: {} -> {}",
                        item.name.as_deref().unwrap_or("unnamed"),
                        local_file_path.display()
                    );
                }
                Err(e) => {
                    warn!(
                        "⚠️ Failed to delete local file {}: {}",
                        local_file_path.display(),
                        e
                    );
                    // Continue processing - don't fail the entire sync cycle
                }
            }
        } else {
            debug!(
                "ℹ️ Local file doesn't exist, skipping deletion: {}",
                local_file_path.display()
            );
        }

        info!(
            "🗑️ File deleted from OneDrive: {} ({}) with inode {}",
            item.name.as_deref().unwrap_or("unnamed"),
            item.id,
            inode
        );
        Ok(())
    }

    /// Handle moving of items
    async fn handle_move_item(
        &self,
        item: &DriveItem,
        drive_item_with_fuse_repo: &DriveItemWithFuseRepository,
    ) -> Result<()> {
        // Update parent reference with Fuse metadata
        let local_path = self
            .app_state
            .config()
            .project_dirs
            .data_dir()
            .join("downloads");
        let inode = self.setup_fuse_metadata(item, drive_item_with_fuse_repo, &local_path).await?;

        // TODO: Handle move logic for "download on demand" later
        info!(
            "📁 File moved: {} ({}) with inode {}",
            item.name.as_deref().unwrap_or("unnamed"),
            item.id,
            inode
        );
        Ok(())
    }

    /// Ensure downloads directory exists
    fn ensure_downloads_directory(path: &std::path::Path) -> Result<()> {
        if !path.exists() {
            std::fs::create_dir_all(path).with_context(|| {
                format!("Failed to create downloads directory: {}", path.display())
            })?;
        }
        Ok(())
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

            // Write downloaded data to local file
            std::fs::write(local_path, download_result.file_data).with_context(|| {
                format!(
                    "Failed to write file {}: {}",
                    local_path.display(),
                    drive_item_id
                )
            })?;

            info!(
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
        let download_folders = self.app_state.config().settings.download_folders.clone();

        info!(
            "🔄 Starting two-way sync cycle with download folders: {:?}",
            download_folders
        );

        // Get delta changes from OneDrive
        let items = self.get_delta_changes().await?;
        info!("📊 Retrieved {} delta items", items.len());
        
        // Create ProcessingItems for remote changes
        let processing_items_repo = ProcessingItemRepository::new(self.app_state.persistency().pool().clone());
        for item in &items {
            let change_operation = self.detect_change_operation(item);
            let processing_item = ProcessingItem::new_remote(item.clone(), change_operation);
            processing_items_repo.store_processing_item(&processing_item).await?;
        }

        // Process all items using the new two-way sync system
        let sync_processor = crate::sync::sync_processor::SyncProcessor::new(self.app_state.clone());
        sync_processor.process_all_items().await?;

        // Process download queue (for backward compatibility)
        self.process_download_queue().await?;

        info!("✅ Two-way sync cycle completed");
        Ok(())
    }

    /// Detect change operation based on OneDrive delta response and existing DB state
    fn detect_change_operation(&self, item: &DriveItem) -> crate::persistency::processing_item_repository::ChangeOperation {
        let drive_item_with_fuse_repo = DriveItemWithFuseRepository::new(self.app_state.persistency().pool().clone());
        
        // Get existing item from DB
        let existing_item = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(
                drive_item_with_fuse_repo.get_drive_item_with_fuse(&item.id)
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
                } else if self.etag_changed(&existing.drive_item, item) {
                    crate::persistency::processing_item_repository::ChangeOperation::Update
                } else {
                    crate::persistency::processing_item_repository::ChangeOperation::Update
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

    /// Remove all child items of a deleted folder from download queue
    async fn remove_child_items_from_download_queue(
        &self,
        parent_id: &str,
        download_queue_repo: &DownloadQueueRepository,
    ) -> Result<()> {
        // Get all items that have this parent_id
        let drive_item_with_fuse_repo = DriveItemWithFuseRepository::new(self.app_state.persistency().pool().clone());
        let child_items = drive_item_with_fuse_repo.get_drive_items_with_fuse_by_parent(parent_id).await?;
        
        let mut removed_count = 0;
        for child_item in child_items {
            if let Err(e) = download_queue_repo.remove_by_drive_item_id(&child_item.drive_item.id).await {
                warn!("⚠️ Failed to remove child item from download queue: {}", e);
            } else {
                removed_count += 1;
            }
        }
        
        if removed_count > 0 {
            info!("📋 Removed {} child items from download queue for deleted folder: {}", removed_count, parent_id);
        }
        
        Ok(())
    }

    /// Delete local files for all child items of a deleted folder
    async fn delete_child_local_files(
        &self,
        parent_id: &str,
        local_path: &std::path::Path,
    ) -> Result<()> {
        // Get all items that have this parent_id
        let drive_item_with_fuse_repo = DriveItemWithFuseRepository::new(self.app_state.persistency().pool().clone());
        let child_items = drive_item_with_fuse_repo.get_drive_items_with_fuse_by_parent(parent_id).await?;
        
        let mut deleted_count = 0;
        for child_item in child_items {
            let child_local_path = local_path.join(child_item.drive_item.id.clone());
            if child_local_path.exists() {
                match std::fs::remove_file(&child_local_path) {
                    Ok(_) => {
                        deleted_count += 1;
                        debug!(
                            "🗑️ Deleted child local file: {} -> {}",
                            child_item.drive_item.name.as_deref().unwrap_or("unnamed"),
                            child_local_path.display()
                        );
                    }
                    Err(e) => {
                        warn!(
                            "⚠️ Failed to delete child local file {}: {}",
                            child_local_path.display(),
                            e
                        );
                    }
                }
            }
        }
        
        if deleted_count > 0 {
            info!("🗑️ Deleted {} child local files for deleted folder: {}", deleted_count, parent_id);
        }
        
        Ok(())
    }
}
