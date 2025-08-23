use crate::app_state::AppState;
use crate::file_manager::FileManager;
use crate::persistency::drive_item_with_fuse_repository::DriveItemWithFuseRepository;
use crate::persistency::processing_item_repository::{
    ChangeOperation, ChangeType, ProcessingItem, ProcessingItemRepository, ProcessingStatus,
};
use crate::sync::sync_strategy::SyncStrategy;
use anyhow::{Context, Result};
use log::{debug, error, info, warn};
use std::path::PathBuf;
use std::sync::Arc;

pub struct SyncProcessor {
    strategy: SyncStrategy,
    app_state: Arc<AppState>,
    processing_repo: ProcessingItemRepository,
    drive_item_with_fuse_repo: DriveItemWithFuseRepository,
}

impl SyncProcessor {
    pub fn new(app_state: Arc<AppState>) -> Self {
        let processing_repo = app_state.persistency().processing_item_repository();
        let drive_item_with_fuse_repo = app_state.persistency().drive_item_with_fuse_repository();
        Self {
            strategy: SyncStrategy::new(app_state.clone()),
            app_state,
            processing_repo,
            drive_item_with_fuse_repo,
        }
    }

    /// Process all items with priority: Remote first, then Local
    pub async fn process_all_items(&self) -> Result<()> {
        debug!("🏠️ Clean up processing items...");
        //self.processing_repo.hause_keeping().await?;

        // NEW: Squash local changes before processing
        debug!("🔀 Squashing local changes...");
        self.squash_local_changes().await?;

        // 1. Process Remote changes first
        debug!("🔄 Processing remote changes...");
        let remote_items = self
            .processing_repo
            .get_unprocessed_items_by_change_type(&ChangeType::Remote)
            .await?;
        for item in remote_items {
            if let Err(e) = self.process_single_item(&item).await {
                error!("❌ Failed to process remote item: {}", e);
                self.processing_repo
                    .update_status_by_id(item.id.unwrap(), &ProcessingStatus::Error)
                    .await?;
            }
        }

        // 2. Process Local changes after remote changes are handled
        debug!("🔄 Processing local changes...");
        loop {
            // Always fetch the next unprocessed local item
            if let Some(item) = self
                .processing_repo
                .get_next_unprocessed_item_by_change_type(&ChangeType::Local)
                .await?
            {
                if let Err(e) = self.process_single_item(&item).await {
                    error!("❌ Failed to process local item: {}", e);
                    self.processing_repo
                        .update_status_by_id(item.id.unwrap(), &ProcessingStatus::Error)
                        .await?;
                }
            } else {
                // No more unprocessed items
                break;
            }
        }

        Ok(())
    }

    /// Squash local changes before processing to consolidate multiple changes into final state
    pub async fn squash_local_changes(&self) -> Result<()> {
        // Get unique drive item IDs with local processing items
        let unique_ids = self.get_unique_drive_items_with_local_changes().await?;
        
        for drive_item_id in unique_ids {
            self.squash_changes_for_drive_item(&drive_item_id).await?;
        }
        
        Ok(())
    }

    /// Squash changes for a specific drive item by applying rules sequentially
    async fn squash_changes_for_drive_item(&self, drive_item_id: &str) -> Result<()> {
        loop {
            // Get CURRENT processing items from database (already ordered by ID)
            let items = self.get_local_processing_items_for_drive_item(drive_item_id).await?;
            
            if items.is_empty() {
                break; // No more items to squash
            }
            
            // Apply rules sequentially
            if self.squash_create_delete_sequence(drive_item_id).await? {
                continue; // Fetch fresh data and restart loop
            }
            
            if self.squash_create_modify_sequence(drive_item_id).await? {
                continue; // Fetch fresh data and restart loop
            }
            
            if self.squash_multiple_modifies(drive_item_id).await? {
                continue; // Fetch fresh data and restart loop
            }
            
            if self.squash_multiple_renames(drive_item_id).await? {
                continue; // Fetch fresh data and restart loop
            }
            
            if self.squash_multiple_moves(drive_item_id).await? {
                continue; // Fetch fresh data and restart loop
            }
            
            // If we get here, no changes were made
            break;
        }
        
        Ok(())
    }

    /// Get unique drive item IDs that have local processing items
    async fn get_unique_drive_items_with_local_changes(&self) -> Result<Vec<String>> {
        let items = self
            .processing_repo
            .get_unprocessed_items_by_change_type(&ChangeType::Local)
            .await?;
        
        let mut unique_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for item in items {
            unique_ids.insert(item.drive_item.id.clone());
        }
        
        Ok(unique_ids.into_iter().collect())
    }

    /// Get local processing items for a drive item, ordered by ID (chronological order)
    async fn get_local_processing_items_for_drive_item(&self, drive_item_id: &str) -> Result<Vec<ProcessingItem>> {
        self.processing_repo
            .get_processing_items_by_drive_item_id_and_change_type(drive_item_id, &ChangeType::Local)
            .await
    }

    /// Rule 1: Create + Delete = Remove all processing items
    async fn squash_create_delete_sequence(&self, drive_item_id: &str) -> Result<bool> {
        let items = self.get_local_processing_items_for_drive_item(drive_item_id).await?;
        
        let has_create = items.iter().any(|item| item.change_operation == ChangeOperation::Create);
        let has_delete = items.iter().any(|item| item.change_operation == ChangeOperation::Delete);
        
        if has_create && has_delete {
            // Remove all processing items for this drive item
            for item in items {
                if let Some(id) = item.id {
                    self.processing_repo.delete_processing_item_by_id(id).await?;
                }
            }
            info!("🗑️ Squashed Create+Delete sequence for item: {}", drive_item_id);
            return Ok(true);
        }
        
        Ok(false)
    }
    
    /// Rule 2: Create + Modifications = Final Create
    async fn squash_create_modify_sequence(&self, drive_item_id: &str) -> Result<bool> {
        let items = self.get_local_processing_items_for_drive_item(drive_item_id).await?;
        
        let has_create = items.iter().any(|item| item.change_operation == ChangeOperation::Create);
        let has_modifications = items.iter().any(|item| 
            matches!(item.change_operation, 
                ChangeOperation::Update | 
                ChangeOperation::Rename { .. } | 
                ChangeOperation::Move { .. }
            )
        );
        
        if has_create && has_modifications {
            // Get the last item ID (highest ID) - this will become our Create
            let last_item_id = items.iter().max_by_key(|item| item.id.unwrap()).unwrap().id.unwrap();
            
            // Get the last item to convert to Create operation
            let last_item = items.iter().find(|item| item.id.unwrap() == last_item_id).unwrap();
            
            // Convert last item to Create operation
            let mut final_create_item = last_item.clone();
            final_create_item.change_operation = ChangeOperation::Create;
            
            // Update the last item to be a Create
            self.processing_repo.update_processing_item(&final_create_item).await?;
            
            // Remove all other processing items
            for item in items {
                if let Some(id) = item.id {
                    if id != last_item_id {
                        self.processing_repo.delete_processing_item_by_id(id).await?;
                    }
                }
            }
            
            info!("🔀 Squashed Create+Modifications: last item {} becomes Create for drive item: {}", 
                  last_item_id, drive_item_id);
            return Ok(true);
        }
        
        Ok(false)
    }
    
    /// Rule 3: Multiple Modifies = Keep only last Modify
    async fn squash_multiple_modifies(&self, drive_item_id: &str) -> Result<bool> {
        let items = self.get_local_processing_items_for_drive_item(drive_item_id).await?;
        
        let modify_items: Vec<_> = items.iter()
            .filter(|item| item.change_operation == ChangeOperation::Update)
            .collect();
        
        if modify_items.len() > 1 {
            // Get the last modify ID (highest ID)
            let last_modify_id = modify_items.iter().max_by_key(|item| item.id.unwrap()).unwrap().id.unwrap();
            
            // Remove all other modify items
            for item in modify_items {
                if let Some(id) = item.id {
                    if id != last_modify_id {
                        self.processing_repo.delete_processing_item_by_id(id).await?;
                    }
                }
            }
            
            info!("🔀 Squashed multiple modifies, keeping last for item: {}", drive_item_id);
            return Ok(true);
        }
        
        Ok(false)
    }
    
    /// Rule 4: Multiple Renames = Keep only last Rename
    async fn squash_multiple_renames(&self, drive_item_id: &str) -> Result<bool> {
        let items = self.get_local_processing_items_for_drive_item(drive_item_id).await?;
        
        let rename_items: Vec<_> = items.iter()
            .filter(|item| matches!(item.change_operation, ChangeOperation::Rename { .. }))
            .collect();
        
        if rename_items.len() > 1 {
            // Get the last rename ID (highest ID)
            let last_rename_id = rename_items.iter().max_by_key(|item| item.id.unwrap()).unwrap().id.unwrap();
            
            // Remove all other rename items
            for item in rename_items {
                if let Some(id) = item.id {
                    if id != last_rename_id {
                        self.processing_repo.delete_processing_item_by_id(id).await?;
                    }
                }
            }
            
            info!("🔀 Squashed multiple renames, keeping last for item: {}", drive_item_id);
            return Ok(true);
        }
        
        Ok(false)
    }
    
    /// Rule 5: Multiple Moves = Keep only last Move
    async fn squash_multiple_moves(&self, drive_item_id: &str) -> Result<bool> {
        let items = self.get_local_processing_items_for_drive_item(drive_item_id).await?;
        
        let move_items: Vec<_> = items.iter()
            .filter(|item| matches!(item.change_operation, ChangeOperation::Move { .. }))
            .collect();
        
        if move_items.len() > 1 {
            // Get the last move ID (highest ID)
            let last_move_id = move_items.iter().max_by_key(|item| item.id.unwrap()).unwrap().id.unwrap();
            
            // Remove all other move items
            for item in move_items {
                if let Some(id) = item.id {
                    if id != last_move_id {
                        self.processing_repo.delete_processing_item_by_id(id).await?;
                    }
                }
            }
            
            info!("🔀 Squashed multiple moves, keeping last for item: {}", drive_item_id);
            return Ok(true);
        }
        
        Ok(false)
    }

    /// Process a single item with validation and conflict resolution
    pub async fn process_single_item(&self, item: &ProcessingItem) -> Result<()> {
        let db_id = item
            .id
            .ok_or_else(|| anyhow::anyhow!("ProcessingItem has no database ID"))?;

        match item.change_type {
            ChangeType::Remote => {
                let mut conflicts = self.strategy.detect_remote_conflicts(item).await?;
                
                // Try to auto-resolve conflicts before marking item as conflicted
                if !conflicts.is_empty() {
                    self.strategy.auto_resolve_remote_conflicts(item, &mut conflicts).await?;
                }
                
                if conflicts.is_empty() {
                    self.processing_repo
                        .update_status_by_id(db_id, &ProcessingStatus::Validated)
                        .await?;
                    self.process_remote_item(item).await?;
                } else {
                    let error_strings: Vec<String> = conflicts.iter().map(|e| e.to_string()).collect();
                    warn!(
                        "Remote conflicts detected for item {}: {:?}",
                        item.drive_item.id, error_strings
                    );
                    self.processing_repo
                        .update_status_by_id(db_id, &ProcessingStatus::Conflicted)
                        .await?;
                    self.processing_repo
                        .update_validation_errors_by_id(db_id, &error_strings)
                        .await?;
                }
            }
            ChangeType::Local => {
                // Before processing a local change, check if a remote change for the same item is already conflicted
                // if let Ok(Some(remote_item)) = self.processing_repo.get_pending_processing_item_by_drive_item_id_and_change_type(&item.drive_item.id, &ChangeType::Remote).await {
                //     if remote_item.status == ProcessingStatus::Conflicted {
                //         self.processing_repo
                //             .update_status_by_id(db_id, &ProcessingStatus::Conflicted)
                //             .await?;
                //         warn!(
                //             "Local change for item {} conflicts with a prior remote change. Both are marked as conflicted.",
                //             item.drive_item.id
                //         );
                //         return Ok(());
                //     }
                // }

                let conflicts = self.strategy.detect_local_conflicts(item).await?;
                if conflicts.is_empty() {
                    self.processing_repo
                        .update_status_by_id(db_id, &ProcessingStatus::Validated)
                        .await?;
                    self.process_local_item(item).await?;
                } else {
                    let error_strings: Vec<String> = conflicts.iter().map(|e| e.to_string()).collect();
                    warn!(
                        "Local conflicts detected for item {}: {:?}",
                        item.drive_item.id, error_strings
                    );
                    self.processing_repo
                        .update_status_by_id(db_id, &ProcessingStatus::Conflicted)
                        .await?;
                    self.processing_repo
                        .update_validation_errors_by_id(db_id, &error_strings)
                        .await?;
                }
            }
        }

        Ok(())
    }

    /// Process a remote item (download, update database, etc.)
    async fn process_remote_item(&self, item: &ProcessingItem) -> Result<()> {
        let processing_repo = self.app_state.persistency().processing_item_repository();
        let db_id = item
            .id
            .ok_or_else(|| anyhow::anyhow!("ProcessingItem has no database ID"))?;
        processing_repo
            .update_status_by_id(db_id, &ProcessingStatus::Processing)
            .await?;
        match item.change_operation {
            ChangeOperation::Create => {
                self.handle_remote_create(item).await?;
            }
            ChangeOperation::Update => {
                self.handle_remote_update(item).await?;
            }
            ChangeOperation::Delete => {
                self.handle_remote_delete(item).await?;
            }
            ChangeOperation::Move { .. } => {
                self.handle_remote_move(item).await?;
            }
            ChangeOperation::Rename { .. } => {
                self.handle_remote_rename(item).await?;
            }
            ChangeOperation::NoChange => {
                error!(
                    "⏭️ No change for item detected : {}",
                    item.drive_item.name.as_deref().unwrap_or("unnamed")
                );
            }
        }
        processing_repo
            .update_status_by_id(db_id, &ProcessingStatus::Done)
            .await?;
        Ok(())
    }

    /// Process a local item (upload to OneDrive, etc.)
    async fn process_local_item(&self, item: &ProcessingItem) -> Result<()> {
        let processing_repo = self.app_state.persistency().processing_item_repository();
        let db_id = item
            .id
            .ok_or_else(|| anyhow::anyhow!("ProcessingItem has no database ID"))?;

        // Skip delete operations - if parent is deleted remotely, item would be too
        if item.change_operation == ChangeOperation::Delete {
            processing_repo
                .update_status_by_id(db_id, &ProcessingStatus::Processing)
                .await?;
            self.handle_local_delete(item).await?;
            processing_repo
                .update_status_by_id(db_id, &ProcessingStatus::Done)
                .await?;
            return Ok(());
        }

        // Check if parent was deleted remotely
        if let Some(parent_ref) = &item.drive_item.parent_reference {
            if self.is_parent_deleted_remotely(&parent_ref.id).await? {
                info!("🔄 Parent deleted remotely, recreating folder structure for: {}", 
                      item.drive_item.name.as_deref().unwrap_or("unnamed"));
                
                // Recreate folder structure using FUSE-style operations
                self.recreate_parent_chain_and_reorder_item(item).await?;
                
                // Skip processing current item - new items will be processed in correct order
                return Ok(());
            }
        }

        processing_repo
            .update_status_by_id(db_id, &ProcessingStatus::Processing)
            .await?;
        match item.change_operation {
            ChangeOperation::Create => {
                self.handle_local_create(item).await?;
            }
            ChangeOperation::Update => {
                self.handle_local_update(item).await?;
            }
            ChangeOperation::Delete => {
                self.handle_local_delete(item).await?;
            }
            ChangeOperation::Move { .. } => {
                self.handle_local_move(item).await?;
            }
            ChangeOperation::Rename { .. } => {
                self.handle_local_rename(item).await?;
            }
            ChangeOperation::NoChange => {
                error!(
                    "⏭️ No change for item detecded from local: {}",
                    item.drive_item.name.as_deref().unwrap_or("unnamed")
                );
            }
        }
        processing_repo
            .update_status_by_id(db_id, &ProcessingStatus::Done)
            .await?;
        Ok(())
    }

    // Remote operation handlers
    async fn handle_remote_create(&self, item: &ProcessingItem) -> Result<()> {
        debug!(
            "📥 Processing remote create: {}",
            item.drive_item.name.as_deref().unwrap_or("unnamed")
        );

        let drive_item_with_fuse_repo = self
            .app_state
            .persistency()
            .drive_item_with_fuse_repository();
        let download_queue_repo = self.app_state.persistency().download_queue_repository();

        // Get local downloads path
        let local_path = self
            .app_state
            .config()
            .project_dirs
            .data_dir()
            .join("downloads");

        // Setup FUSE metadata and store the item
        let inode = self
            .setup_fuse_metadata(&item.drive_item, &drive_item_with_fuse_repo, &local_path)
            .await?;

        info!(
            "📁 Created remote item: {} ({}) with inode {}",
            item.drive_item.name.as_deref().unwrap_or("unnamed"),
            item.drive_item.id,
            inode
        );

        // Add to download queue if it should be downloaded
        if self.should_download(&item.drive_item).await {
            let local_file_path = local_path.join(item.drive_item.id.clone());
            download_queue_repo
                .add_to_download_queue(&item.drive_item.id, &local_file_path)
                .await?;
            info!(
                "📥 Added new remote file to download queue: {} ({})",
                item.drive_item.name.as_deref().unwrap_or("unnamed"),
                item.drive_item.id
            );
        }

        Ok(())
    }

    async fn handle_remote_update(&self, item: &ProcessingItem) -> Result<()> {
        debug!(
            "📝 Processing remote update: {}",
            item.drive_item.name.as_deref().unwrap_or("unnamed")
        );

        let drive_item_with_fuse_repo = self
            .app_state
            .persistency()
            .drive_item_with_fuse_repository();
        let download_queue_repo = self.app_state.persistency().download_queue_repository();

        // Get local downloads path
        let local_path = self
            .app_state
            .config()
            .project_dirs
            .data_dir()
            .join("downloads");

        // Get existing item to check if etag changed
        let existing_item = drive_item_with_fuse_repo
            .get_drive_item_with_fuse(&item.drive_item.id)
            .await?;

        // Setup FUSE metadata and store the updated item
        let inode = self
            .setup_fuse_metadata(&item.drive_item, &drive_item_with_fuse_repo, &local_path)
            .await?;

        info!(
            "📝 Updated remote item: {} ({}) with inode {}",
            item.drive_item.name.as_deref().unwrap_or("unnamed"),
            item.drive_item.id,
            inode
        );

        // Check if etag changed and file should be downloaded
        if let Some(existing) = &existing_item {
            if self.etag_changed(&existing.drive_item, &item.drive_item)
                && self.should_download(&item.drive_item).await
            {
                let local_file_path = local_path.join(item.drive_item.id.clone());
                download_queue_repo
                    .add_to_download_queue(&item.drive_item.id, &local_file_path)
                    .await?;
                debug!(
                    "📥 Added modified remote file to download queue: {} ({})",
                    item.drive_item.name.as_deref().unwrap_or("unnamed"),
                    item.drive_item.id
                );
            }
        }

        Ok(())
    }

    async fn handle_remote_delete(&self, item: &ProcessingItem) -> Result<()> {
        info!(
            "🗑️ Processing remote delete: {}",
            item.drive_item.name.as_deref().unwrap_or("unnamed")
        );

        let drive_item_with_fuse_repo = self
            .app_state
            .persistency()
            .drive_item_with_fuse_repository();
        let download_queue_repo = self.app_state.persistency().download_queue_repository();

        // Get local downloads path
        let local_path = self
            .app_state
            .config()
            .project_dirs
            .data_dir()
            .join("downloads");
        let local_file_path = local_path.join(item.drive_item.id.clone());

        // Remove item from download queue if it exists
        if let Err(e) = download_queue_repo
            .remove_by_drive_item_id(&item.drive_item.id)
            .await
        {
            warn!("⚠️ Failed to remove item from download queue: {}", e);
        } else {
            debug!(
                "📋 Removed deleted item from download queue: {}",
                item.drive_item.id
            );
        }

        // If it's a folder, also remove all child items from download queue and delete their local files
        if item.drive_item.folder.is_some() {
            self.remove_child_items_from_download_queue(&item.drive_item.id, &download_queue_repo)
                .await?;
            self.delete_child_local_files(&item.drive_item.id, &local_path)
                .await?;
        }

        // Remove item from drive_items_with_fuse table
        if let Err(e) = drive_item_with_fuse_repo
            .mark_as_deleted_by_onedrive_id(&item.drive_item.id)
            .await
        {
            warn!("⚠️ Failed to remove item from drive_items_with_fuse: {}", e);
        } else {
            debug!(
                "🗑️ Removed item from drive_items_with_fuse: {}",
                item.drive_item.id
            );
        }

        // Delete local file if it exists
        if local_file_path.exists() {
            match std::fs::remove_file(&local_file_path) {
                Ok(_) => {
                    info!(
                        "🗑️ Deleted local file: {} -> {}",
                        item.drive_item.name.as_deref().unwrap_or("unnamed"),
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

        debug!(
            "🗑️ File deleted from OneDrive: {} ({})",
            item.drive_item.name.as_deref().unwrap_or("unnamed"),
            item.drive_item.id
        );

        Ok(())
    }

    async fn handle_remote_move(&self, item: &ProcessingItem) -> Result<()> {
        debug!(
            "📁 Processing remote move: {}",
            item.drive_item.name.as_deref().unwrap_or("unnamed")
        );

        let drive_item_with_fuse_repo = self
            .app_state
            .persistency()
            .drive_item_with_fuse_repository();

        // Get the new parent ID from the processing item
        let new_parent_id = if let Some(parent_ref) = &item.drive_item.parent_reference {
            &parent_ref.id
        } else {
            return Err(anyhow::anyhow!(
                "No parent reference specified for move operation"
            ));
        };

        // Move the item on OneDrive
        match self
            .app_state
            .onedrive_client
            .move_item(&item.drive_item.id, new_parent_id)
            .await
        {
            Ok(moved_item) => {
                info!(
                    "📁 Moved item on OneDrive: {} -> parent: {}",
                    item.drive_item.id, new_parent_id
                );

                // Update the processing item with the moved item data
                let mut updated_item = item.drive_item.clone();
                updated_item.id = moved_item.id;
                updated_item.etag = moved_item.etag;
                updated_item.parent_reference = moved_item.parent_reference;

                // Setup FUSE metadata for the moved item
                let local_downloads_path = self
                    .app_state
                    .config()
                    .project_dirs
                    .data_dir()
                    .join("downloads");
                let _inode = self
                    .setup_fuse_metadata(
                        &updated_item,
                        &drive_item_with_fuse_repo,
                        &local_downloads_path,
                    )
                    .await?;
            }
            Err(e) => {
                error!("❌ Failed to move item on OneDrive: {}", e);
                return Err(e);
            }
        }

        Ok(())
    }

    async fn handle_remote_rename(&self, item: &ProcessingItem) -> Result<()> {
        debug!(
            "🏷️ Processing remote rename: {}",
            item.drive_item.name.as_deref().unwrap_or("unnamed")
        );

        let drive_item_with_fuse_repo = self
            .app_state
            .persistency()
            .drive_item_with_fuse_repository();

        // Get local downloads path
        let local_path = self
            .app_state
            .config()
            .project_dirs
            .data_dir()
            .join("downloads");

        // Setup FUSE metadata and update the item with new name
        let inode = self
            .setup_fuse_metadata(&item.drive_item, &drive_item_with_fuse_repo, &local_path)
            .await?;

        info!(
            "🏷️ File renamed: {} ({}) with inode {}",
            item.drive_item.name.as_deref().unwrap_or("unnamed"),
            item.drive_item.id,
            inode
        );

        Ok(())
    }

    // Local operation handlers
    async fn handle_local_create(&self, item: &ProcessingItem) -> Result<()> {
        debug!(
            "📤 Processing local create: {}",
            item.drive_item.name.as_deref().unwrap_or("unnamed")
        );
        // get the actual Fuse Item
        let fuse_item = self
            .drive_item_with_fuse_repo
            .get_drive_item_with_fuse(&item.drive_item.id)
            .await
            .context("Failed to get FUSE item")?
            .unwrap();

        // Get local path from the processing item
        let local_path = self
            .app_state
            .file_manager()
            .get_local_dir()
            .join(&fuse_item.virtual_ino().unwrap().to_string());

        // Check if it's a folder or file
        if item.drive_item.folder.is_some() {
            // For folders, create on OneDrive and get real OneDrive ID
            let folder_name = item.drive_item.name.as_deref().unwrap_or("unnamed");
            let parent_path = self.get_parent_path_from_item(&item.drive_item)?;

            match self
                .app_state
                .onedrive_client
                .create_folder(&parent_path, folder_name)
                .await
            {
                Ok(result) => {
                    info!(
                        "📁 Created folder on OneDrive: {} -> {}",
                        folder_name, result.onedrive_id
                    );

                    // Update all database references from temporary ID to real OneDrive ID
                    let temporary_id = &item.drive_item.id;
                    let real_onedrive_id = &result.onedrive_id;

                    // Update DriveItemWithFuse
                    self.drive_item_with_fuse_repo
                        .update_onedrive_id(temporary_id, real_onedrive_id)
                        .await?;

                    // Update ProcessingItems
                    self.processing_repo
                        .update_onedrive_id(temporary_id, real_onedrive_id)
                        .await?;

                    // Update parent IDs for any children that reference this temporary ID
                    self.drive_item_with_fuse_repo
                        .update_parent_id_for_children(temporary_id, real_onedrive_id)
                        .await?;
                    self.processing_repo
                        .update_parent_id_for_children(temporary_id, real_onedrive_id)
                        .await?;

                    debug!(
                        "🔄 Updated database references: {} -> {}",
                        temporary_id, real_onedrive_id
                    );

                    // Get the full DriveItem from OneDrive to update with complete metadata
                    match self
                        .app_state
                        .onedrive_client
                        .get_item_by_id(real_onedrive_id)
                        .await
                    {
                        Ok(full_drive_item) => {
                            // Setup FUSE metadata for the created folder with real OneDrive data
                            let local_downloads_path = self
                                .app_state
                                .config()
                                .project_dirs
                                .data_dir()
                                .join("downloads");
                            let _inode = self
                                .setup_fuse_metadata(
                                    &full_drive_item,
                                    &self.drive_item_with_fuse_repo,
                                    &local_downloads_path,
                                )
                                .await?;

                            // Update the processing item with the real OneDrive data
                            let mut updated_processing_item = item.clone();
                            updated_processing_item.drive_item = full_drive_item;
                            self.processing_repo
                                .update_processing_item(&updated_processing_item)
                                .await?;

                            debug!(
                                "✅ Updated processing item with real OneDrive data for folder: {}",
                                folder_name
                            );
                        }
                        Err(e) => {
                            warn!("⚠️ Failed to get full DriveItem for created folder: {}", e);
                            // Continue anyway since we have the basic info
                        }
                    }
                }
                Err(e) => {
                    error!("❌ Failed to create folder on OneDrive: {}", e);
                    return Err(e);
                }
            }
        } else {
            // Handle file creation - upload immediately and get real OneDrive ID
            let file_name = item.drive_item.name.as_deref().unwrap_or("unnamed");

            // Get parent ID for the correct API endpoint
            let parent_id = if let Some(parent_ref) = &item.drive_item.parent_reference {
                parent_ref.id.clone()
            } else {
                return Err(anyhow::anyhow!(
                    "No parent reference specified for local create operation"
                ));
            };

            // Read file data from local path
            if local_path.exists() {
                match std::fs::read(&local_path) {
                    Ok(file_data) => {
                        // Use the correct API endpoint with parent ID
                        match self
                            .app_state
                            .onedrive_client
                            .upload_new_file_to_parent(&file_data, file_name, &parent_id)
                            .await
                        {
                            Ok(result) => {
                                info!(
                                    "📤 Uploaded file to OneDrive: {} -> {}",
                                    file_name, result.onedrive_id
                                );

                                // Update all database references from temporary ID to real OneDrive ID
                                let temporary_id = &item.drive_item.id;
                                let real_onedrive_id = &result.onedrive_id;

                                // Update DriveItemWithFuse
                                self.drive_item_with_fuse_repo
                                    .update_onedrive_id(temporary_id, real_onedrive_id)
                                    .await?;

                                // Update ProcessingItems
                                self.processing_repo
                                    .update_onedrive_id(temporary_id, real_onedrive_id)
                                    .await?;

                                // Update parent IDs for any children that reference this temporary ID
                                self.drive_item_with_fuse_repo
                                    .update_parent_id_for_children(temporary_id, real_onedrive_id)
                                    .await?;
                                self.processing_repo
                                    .update_parent_id_for_children(temporary_id, real_onedrive_id)
                                    .await?;

                                debug!(
                                    "🔄 Updated database references: {} -> {}",
                                    temporary_id, real_onedrive_id
                                );

                                // Get the full DriveItem from OneDrive to update with complete metadata
                                match self
                                    .app_state
                                    .onedrive_client
                                    .get_item_by_id(real_onedrive_id)
                                    .await
                                {
                                    Ok(full_drive_item) => {
                                        // Move file from upload folder to download folder
                                        if let Err(e) = self
                                            .move_file_to_its_new_name(
                                                &local_path,
                                                real_onedrive_id,
                                            )
                                            .await
                                        {
                                            warn!(
                                                "⚠️ Failed to move file to download folder: {}",
                                                e
                                            );
                                        }

                                        // Setup FUSE metadata for the uploaded file with real OneDrive data
                                        let local_downloads_path = self
                                            .app_state
                                            .config()
                                            .project_dirs
                                            .data_dir()
                                            .join("downloads");
                                        let _inode = self
                                            .setup_fuse_metadata(
                                                &full_drive_item,
                                                &self.drive_item_with_fuse_repo,
                                                &local_downloads_path,
                                            )
                                            .await?;

                                        // Update the processing item with the real OneDrive data
                                        let mut updated_processing_item = item.clone();
                                        updated_processing_item.drive_item = full_drive_item;
                                        self.processing_repo
                                            .update_processing_item(&updated_processing_item)
                                            .await?;

                                        // NEW: Store ctag from upload result
                                        if let Some(ctag) = &result.ctag {
                                            self.drive_item_with_fuse_repo
                                                .update_ctag(real_onedrive_id, ctag)
                                                .await?;
                                            debug!("✅ Stored ctag for uploaded file: {} -> {}", file_name, ctag);
                                        }

                                        debug!("✅ Updated processing item with real OneDrive data for file: {}", file_name);
                                    }
                                    Err(e) => {
                                        warn!(
                                            "⚠️ Failed to get full DriveItem for uploaded file: {}",
                                            e
                                        );
                                        // Continue anyway since we have the basic info
                                    }
                                }
                            }
                            Err(e) => {
                                error!("❌ Failed to upload file to OneDrive: {}", e);
                                return Err(e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("❌ Failed to read local file for upload: {}", e);
                        return Err(anyhow::anyhow!("Failed to read local file: {}", e));
                    }
                }
            } else {
                return Err(anyhow::anyhow!(
                    "Local file does not exist: {}",
                    local_path.display()
                ));
            }
        }

        Ok(())
    }

    async fn handle_local_update(&self, item: &ProcessingItem) -> Result<()> {
        debug!(
            "📤 Processing local update: {}",
            item.drive_item.name.as_deref().unwrap_or("unnamed")
        );

        // Get local path using inode from database
        let mut fs = self
            .drive_item_with_fuse_repo
            .get_drive_item_with_fuse(&item.drive_item.id)
            .await
            .context("Failed to obtain FUSE item")?
            .unwrap();
        let path = self
            .app_state
            .file_manager()
            .get_local_dir()
            .join(fs.virtual_ino().unwrap().to_string());
        if !path.exists() {
            return Err(anyhow::anyhow!(
                "Local file does not exist: {}",
                path.display()
            ));
        }

        // Check if it's a folder or file
        if item.drive_item.folder.is_some() {
            // For folders, just update metadata (no content to update)
            let local_downloads_path = self
                .app_state
                .config()
                .project_dirs
                .data_dir()
                .join("downloads");
            let _inode = self
                .setup_fuse_metadata(
                    &item.drive_item,
                    &self.drive_item_with_fuse_repo,
                    &local_downloads_path,
                )
                .await?;
            debug!(
                "📁 Updated folder metadata: {}",
                item.drive_item.name.as_deref().unwrap_or("unnamed")
            );
        } else {
            // For files, read the file and update on OneDrive

            if path.exists() {
                let file_data = std::fs::read(&path).context("Failed to read local file")?;
                let result = self
                    .app_state
                    .onedrive_client
                    .upload_updated_file(&file_data, &item.drive_item.id)
                    .await
                    .context("Failed to update file on OneDrive")?;
                info!(
                    "📤 Updated file on OneDrive: {} -> {}",
                    path.display(),
                    result.onedrive_id
                );
                fs.set_sync_status("synced".to_string());

                fs.drive_item.set_etag(result.etag.clone().unwrap());
                fs.drive_item.set_size(result.size.clone().unwrap());
                fs.drive_item.set_ctag(result.ctag.clone().unwrap());
                self.drive_item_with_fuse_repo
                    .store_drive_item_with_fuse(&fs)
                    .await
                    .context("Failed to store modifiedFUSE item")?;
            } else {
                return Err(anyhow::anyhow!(
                    "Local file does not exist: {}",
                    path.display()
                ));
            }
        }

        Ok(())
    }

    async fn handle_local_delete(&self, item: &ProcessingItem) -> Result<()> {
        debug!(
            "🗑️ Processing local delete: {}",
            item.drive_item.name.as_deref().unwrap_or("unnamed")
        );

        let drive_item_with_fuse_repo = self
            .app_state
            .persistency()
            .drive_item_with_fuse_repository();

        // Get the virtual path for the item to delete
        let virtual_path = if let Some(existing_item) = drive_item_with_fuse_repo
            .get_drive_item_with_fuse(&item.drive_item.id)
            .await?
        {
            existing_item.virtual_path().unwrap_or_default().to_string()
        } else {
            return Err(anyhow::anyhow!(
                "Item not found in FUSE database for deletion"
            ));
        };

        // Delete from OneDrive using the virtual path
        match self
            .app_state
            .onedrive_client
            .delete_item(&virtual_path)
            .await
        {
            Ok(result) => {
                info!(
                    "🗑️ Deleted item from OneDrive: {} -> {}",
                    virtual_path, result.item_path
                );

                // Mark as deleted in FUSE database
                let local_downloads_path = self
                    .app_state
                    .config()
                    .project_dirs
                    .data_dir()
                    .join("downloads");
                let _inode = self
                    .setup_fuse_metadata(
                        &item.drive_item,
                        &drive_item_with_fuse_repo,
                        &local_downloads_path,
                    )
                    .await?;
            }
            Err(e) => {
                error!("❌ Failed to delete item from OneDrive: {}", e);
                return Err(e);
            }
        }

        Ok(())
    }

    async fn handle_local_move(&self, item: &ProcessingItem) -> Result<()> {
        debug!(
            "📁 Processing local move: {}",
            item.drive_item.name.as_deref().unwrap_or("unnamed")
        );

        let drive_item_with_fuse_repo = self
            .app_state
            .persistency()
            .drive_item_with_fuse_repository();
        let processing_repo = self.app_state.persistency().processing_item_repository();

        // Get the new parent ID from the processing item
        let new_parent_id = if let Some(parent_ref) = &item.drive_item.parent_reference {
            &parent_ref.id
        } else {
            return Err(anyhow::anyhow!(
                "No parent reference specified for move operation"
            ));
        };

        // Move the item on OneDrive
        match self
            .app_state
            .onedrive_client
            .move_item(&item.drive_item.id, new_parent_id)
            .await
        {
            Ok(moved_item) => {
                info!(
                    "📁 Moved item on OneDrive: {} -> parent: {}",
                    item.drive_item.id, new_parent_id
                );

                // Setup FUSE metadata for the moved item with real OneDrive data
                let local_downloads_path = self
                    .app_state
                    .config()
                    .project_dirs
                    .data_dir()
                    .join("downloads");
                let _inode = self
                    .setup_fuse_metadata(
                        &moved_item,
                        &drive_item_with_fuse_repo,
                        &local_downloads_path,
                    )
                    .await?;

                // Update the processing item with the real OneDrive data
                let mut updated_processing_item = item.clone();
                updated_processing_item.drive_item = moved_item;
                processing_repo
                    .update_processing_item(&updated_processing_item)
                    .await?;

                debug!(
                    "✅ Updated processing item with real OneDrive data for moved item: {}",
                    item.drive_item.name.as_deref().unwrap_or("unnamed")
                );
            }
            Err(e) => {
                error!("❌ Failed to move item on OneDrive: {}", e);
                return Err(e);
            }
        }

        Ok(())
    }

    async fn handle_local_rename(&self, item: &ProcessingItem) -> Result<()> {
        debug!(
            "🏷️ Processing local rename: {}",
            item.drive_item.name.as_deref().unwrap_or("unnamed")
        );

        let drive_item_with_fuse_repo = self
            .app_state
            .persistency()
            .drive_item_with_fuse_repository();
        let processing_repo = self.app_state.persistency().processing_item_repository();

        // Get the new name from the processing item
        let new_name = if let Some(name) = &item.drive_item.name {
            name
        } else {
            return Err(anyhow::anyhow!("No name specified for rename operation"));
        };

        // Rename the item on OneDrive
        match self
            .app_state
            .onedrive_client
            .rename_item(&item.drive_item.id, new_name)
            .await
        {
            Ok(renamed_item) => {
                info!(
                    "🏷️ Renamed item on OneDrive: {} -> {}",
                    item.drive_item.id, new_name
                );

                // Setup FUSE metadata for the renamed item with real OneDrive data
                let local_downloads_path = self
                    .app_state
                    .config()
                    .project_dirs
                    .data_dir()
                    .join("downloads");
                let _inode = self
                    .setup_fuse_metadata(
                        &renamed_item,
                        &drive_item_with_fuse_repo,
                        &local_downloads_path,
                    )
                    .await?;

                // Update the processing item with the real OneDrive data
                let mut updated_processing_item = item.clone();
                updated_processing_item.drive_item = renamed_item;
                processing_repo
                    .update_processing_item(&updated_processing_item)
                    .await?;

                debug!(
                    "✅ Updated processing item with real OneDrive data for renamed item: {}",
                    new_name
                );
            }
            Err(e) => {
                error!("❌ Failed to rename item on OneDrive: {}", e);
                return Err(e);
            }
        }

        Ok(())
    }



    // Helper methods adapted from delta_update.rs
    async fn setup_fuse_metadata(
        &self,
        item: &crate::onedrive_service::onedrive_models::DriveItem,
        drive_item_with_fuse_repo: &crate::persistency::drive_item_with_fuse_repository::DriveItemWithFuseRepository,
        local_path: &std::path::Path,
    ) -> Result<u64> {
        // Check if item already exists to preserve its inode
        let existing_item = drive_item_with_fuse_repo
            .get_drive_item_with_fuse(&item.id)
            .await?;

        // Create the item with basic FUSE metadata
        let mut item_with_fuse = drive_item_with_fuse_repo.create_from_drive_item(item.clone());

        // Set file source to Remote since this comes from OneDrive
        item_with_fuse.set_file_source(crate::persistency::types::FileSource::Remote);
        item_with_fuse.set_sync_status("synced".to_string());

        // Set local path for downloaded files
        let local_file_path = local_path.join(item.id.clone());

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
            if let Ok(Some(parent_item)) = drive_item_with_fuse_repo
                .get_drive_item_with_fuse(parent_id)
                .await
            {
                if let Some(parent_ino) = parent_item.virtual_ino() {
                    item_with_fuse.set_parent_ino(parent_ino);
                }
            }
        }

        // Store the item and get the inode (preserved or new)
        let inode = drive_item_with_fuse_repo
            .store_drive_item_with_fuse(&item_with_fuse)
            .await?;

        Ok(inode)
    }

    /// Determines if a file should be downloaded based on its parent folder path
    ///
    /// # Arguments
    /// * `item` - The OneDrive item to check
    ///
    /// # Returns
    /// * `true` if the item should be downloaded, `false` otherwise
    ///
    /// # Logic
    /// 1. Skip folders (download on demand)
    /// 2. If no download folders configured, download everything
    /// 3. Check if item's parent path matches any configured download folder
    /// 4. Path matching strips "/drive/root:/" prefix and uses exact folder matching
    async fn should_download(
        &self,
        item: &crate::onedrive_service::onedrive_models::DriveItem,
    ) -> bool {
        // Get configured download folders from settings
        let download_folders = self
            .app_state
            .config()
            .settings
            .read()
            .await
            .download_folders
            .clone();

        // Skip folders - they are downloaded on demand when accessed
        if item.folder.is_some() {
            debug!(
                "📁 Skipping folder for download: {}",
                item.name.as_deref().unwrap_or("unnamed")
            );
            return false;
        }

        // If no download folders specified, download all files
        if download_folders.is_empty() {
            debug!("📥 No download folders configured, downloading all files");
            return true;
        }

        // Check if item's parent path matches any configured download folder
        if let Some(parent_ref) = &item.parent_reference {
            if let Some(parent_path) = &parent_ref.path {
                // Strip "/drive/root:/" prefix to get the virtual path
                // Example: "/drive/root:/Test/Downloads" -> "/Test/Downloads"
                const DRIVE_ROOT_PREFIX: &str = "/drive/root:/";
                let virtual_path = parent_path
                    .strip_prefix(DRIVE_ROOT_PREFIX)
                    .unwrap_or(parent_path);

                // Check if any download folder matches as a prefix (exact folder matching)
                for folder in &download_folders {
                    if virtual_path.starts_with(folder) {
                        debug!(
                            "📥 File matches download folder '{}': {} (path: {})",
                            folder,
                            item.name.as_deref().unwrap_or("unnamed"),
                            virtual_path
                        );
                        return true;
                    }
                }

                debug!(
                    "⏭️ File does not match any download folder: {} (path: {})",
                    item.name.as_deref().unwrap_or("unnamed"),
                    virtual_path
                );
            } else {
                debug!(
                    "⚠️ No parent path available for item: {}",
                    item.name.as_deref().unwrap_or("unnamed")
                );
            }
        } else {
            debug!(
                "⚠️ No parent reference available for item: {}",
                item.name.as_deref().unwrap_or("unnamed")
            );
        }

        false
    }

    fn etag_changed(
        &self,
        existing: &crate::onedrive_service::onedrive_models::DriveItem,
        updated: &crate::onedrive_service::onedrive_models::DriveItem,
    ) -> bool {
        existing.etag != updated.etag
    }

    async fn remove_child_items_from_download_queue(
        &self,
        parent_id: &str,
        download_queue_repo: &crate::persistency::download_queue_repository::DownloadQueueRepository,
    ) -> Result<()> {
        // Get all pending downloads and check if they belong to the deleted parent
        let pending_downloads = download_queue_repo.get_pending_downloads().await?;
        for (queue_id, drive_item_id, _local_path) in pending_downloads {
            // Check if this item is a child of the deleted parent
            let drive_item_with_fuse_repo = self
                .app_state
                .persistency()
                .drive_item_with_fuse_repository();
            if let Ok(Some(item)) = drive_item_with_fuse_repo
                .get_drive_item_with_fuse(&drive_item_id)
                .await
            {
                if item
                    .drive_item
                    .parent_reference
                    .as_ref()
                    .map(|p| p.id.as_str())
                    == Some(parent_id)
                {
                    download_queue_repo
                        .remove_by_drive_item_id(&drive_item_id)
                        .await?;
                    debug!(
                        "📋 Removed child item from download queue: {}",
                        drive_item_id
                    );
                }
            }
        }
        Ok(())
    }

    async fn delete_child_local_files(
        &self,
        parent_id: &str,
        local_path: &std::path::Path,
    ) -> Result<()> {
        let drive_item_with_fuse_repo = self
            .app_state
            .persistency()
            .drive_item_with_fuse_repository();
        let child_items = drive_item_with_fuse_repo
            .get_all_drive_items_with_fuse()
            .await?;
        for item in child_items {
            if item
                .drive_item
                .parent_reference
                .as_ref()
                .map(|p| p.id.as_str())
                == Some(parent_id)
            {
                let local_file_path = local_path.join(item.drive_item.id.clone());
                if local_file_path.exists() {
                    match std::fs::remove_file(&local_file_path) {
                        Ok(_) => {
                            debug!(
                                "🗑️ Deleted local file: {} -> {}",
                                item.drive_item.name.as_deref().unwrap_or("unnamed"),
                                local_file_path.display()
                            );
                        }
                        Err(e) => {
                            warn!(
                                "⚠️ Failed to delete local file {}: {}",
                                local_file_path.display(),
                                e
                            );
                        }
                    }
                } else {
                    debug!(
                        "ℹ️ Local file doesn't exist, skipping deletion: {}",
                        local_file_path.display()
                    );
                }
            }
        }
        Ok(())
    }

    fn get_parent_path_from_item(
        &self,
        item: &crate::onedrive_service::onedrive_models::DriveItem,
    ) -> Result<String> {
        let mut parent_path = String::new();
        if let Some(parent_ref) = &item.parent_reference {
            if let Some(path) = &parent_ref.path {
                parent_path = path.clone();
            }
        }
        if parent_path.is_empty() {
            parent_path = "/".to_string();
        }
        Ok(parent_path)
    }

    // /// Safely move a file from upload folder to download folder
    async fn move_file_to_its_new_name(&self, old_path: &PathBuf, onedrive_id: &str) -> Result<()> {
        // Get the FUSE item to get the ino
        let drive_item_with_fuse_repo = self
            .app_state
            .persistency()
            .drive_item_with_fuse_repository();
        let fuse_item = drive_item_with_fuse_repo
            .get_drive_item_with_fuse(onedrive_id)
            .await?;

        let ino = if let Some(item) = fuse_item {
            item.virtual_ino().unwrap_or(0)
        } else {
            return Err(anyhow::anyhow!(
                "FUSE item not found for onedrive_id: {}",
                onedrive_id
            ));
        };

        let destination_path = self
            .app_state
            .file_manager()
            .get_local_dir()
            .join(ino.to_string());

        // Move file from upload to download
        match std::fs::rename(old_path, &destination_path) {
            Ok(_) => {
                debug!(
                    "📁 Moved file from upload to download: {} -> {}",
                    old_path.display(),
                    destination_path.display()
                );
                Ok(())
            }
            Err(e) => {
                warn!(
                    "⚠️ Failed to move file from upload to download: {} -> {}: {}",
                    old_path.display(),
                    destination_path.display(),
                    e
                );

                // Try to clean up the upload file if move failed
                if let Err(cleanup_err) = std::fs::remove_file(old_path) {
                    warn!(
                        "⚠️ Failed to clean up upload file after move failure: {}: {}",
                        old_path.display(),
                        cleanup_err
                    );
                }

                Err(anyhow::anyhow!("Failed to move file: {}", e))
            }
        }
    }

    // Parent Chain Recreation Methods

    /// Check if parent was deleted remotely
    async fn is_parent_deleted_remotely(&self, parent_id: &str) -> Result<bool> {
        if let Ok(Some(local_parent)) = self.drive_item_with_fuse_repo
            .get_drive_item_with_fuse(parent_id)
            .await 
        {
            Ok(local_parent.is_deleted())
        } else {
            Ok(true)  // Parent doesn't exist locally = deleted
        }
    }

    /// Recreate parent chain and reorder processing item
    async fn recreate_parent_chain_and_reorder_item(&self, item: &ProcessingItem) -> Result<()> {
        // 1. Recursively recreate missing parent folders using FUSE-style operations
        let new_parent_ino = self.recreate_parent_chain_with_fuse_pattern(
            &item.drive_item.parent_reference.as_ref().unwrap().id
        ).await?;
        
        // 2. Update current item with new parent
        self.update_item_parent_and_reorder(item, new_parent_ino).await?;
        
        Ok(())
    }

    /// Recursively recreate parent chain using FUSE-style database operations
    fn recreate_parent_chain_with_fuse_pattern<'a>(&'a self, deleted_parent_id: &'a str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + 'a>> {
        Box::pin(async move {
            // Get the deleted parent from local DB
            let deleted_parent = self.drive_item_with_fuse_repo
                .get_drive_item_with_fuse(deleted_parent_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Deleted parent not found in local DB: {}", deleted_parent_id))?;
            
            // Determine parent inode for this folder
            let parent_ino = if let Some(grandparent_ref) = &deleted_parent.drive_item.parent_reference {
                // Check if grandparent also needs recreation
                if self.is_parent_deleted_remotely(&grandparent_ref.id).await? {
                    // Recursively recreate grandparent first
                    self.recreate_parent_chain_with_fuse_pattern(&grandparent_ref.id).await?
                } else {
                    // Grandparent exists, get its inode
                    self.drive_item_with_fuse_repo
                        .get_drive_item_with_fuse(&grandparent_ref.id)
                        .await?
                        .unwrap()
                        .virtual_ino()
                        .unwrap()
                }
            } else {
                // No grandparent = root folder
                1 // Root inode
            };
            
            // Create new folder using FUSE-style database operation
            let folder_name = deleted_parent.drive_item.name.as_deref().unwrap_or("unnamed");
            let new_ino = self.create_folder_with_fuse_pattern(parent_ino, folder_name).await?;
            
            info!("📁 Created folder in chain: {} -> inode {}", folder_name, new_ino);
            
            Ok(new_ino)
        })
    }

    /// Apply local folder creation (mimics FUSE apply_local_change_to_db_repository)
    async fn apply_local_folder_creation(&self, parent_ino: u64, name: &str) -> Result<u64> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        use std::time::SystemTime;

        // Generate temporary ID
        let mut hasher = DefaultHasher::new();
        SystemTime::now().hash(&mut hasher);
        let temporary_id = format!("local_{:x}", hasher.finish());

        // Get parent item to extract parent_id and parent_path
        let parent_item = self.drive_item_with_fuse_repo
            .get_drive_item_with_fuse_by_virtual_ino(parent_ino)
            .await?;
        let parent_id = parent_item.as_ref().map(|p| p.id().to_string());
        let parent_path = parent_item
            .as_ref()
            .and_then(|p| p.virtual_path())
            .map(|p| format!("/drive/root:{}", p.to_string()));

        // Create a new DriveItem for the local folder
        let drive_item = crate::onedrive_service::onedrive_models::DriveItem {
            id: temporary_id.clone(),
            name: Some(name.to_string()),
            etag: None,
            ctag: None,
            last_modified: Some(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
            created_date: Some(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
            size: Some(0),
            folder: Some(crate::onedrive_service::onedrive_models::FolderFacet { child_count: 0 }),
            file: None,
            download_url: None,
            deleted: None,
            parent_reference: parent_id.as_ref().map(|id| {
                crate::onedrive_service::onedrive_models::ParentReference {
                    id: id.clone(),
                    path: parent_path.clone(),
                }
            }),
        };

        let mut item_with_fuse = self.drive_item_with_fuse_repo
            .create_from_drive_item(drive_item.clone());
        item_with_fuse.set_parent_ino(parent_ino);
        item_with_fuse.set_file_source(crate::persistency::types::FileSource::Local);
        item_with_fuse.set_sync_status("local_change".to_string());

        // Store the item and get the inode
        let inode = self.drive_item_with_fuse_repo
            .store_drive_item_with_fuse(&item_with_fuse)
            .await?;

        debug!(
            "📝 Applied local folder creation: parent_ino={}, name={}, ino={}",
            parent_ino, name, inode
        );

        Ok(inode)
    }

    /// Create folder using FUSE-style database operations
    async fn create_folder_with_fuse_pattern(&self, parent_ino: u64, name: &str) -> Result<u64> {
        // Step 1: Create the folder directly using the same logic as FUSE apply_local_change_to_db_repository
        let new_ino = self.apply_local_folder_creation(parent_ino, name).await?;
        
        // Step 2: Get the created item
        let created_item = self.drive_item_with_fuse_repo
            .get_drive_item_with_fuse_by_virtual_ino(new_ino)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Failed to get created folder"))?;
        
        // Step 3: create_processing_item_for_handle (like FUSE mkdir)
        self.create_processing_item_for_folder(&created_item).await?;
        
        debug!("📁 Created folder using FUSE pattern: {} -> inode {}", name, new_ino);
        
        Ok(new_ino)
    }

    /// Create processing item for folder (following FUSE pattern)
    async fn create_processing_item_for_folder(&self, item: &crate::persistency::types::DriveItemWithFuse) -> Result<()> {
        let onedrive_id = &item.drive_item().id;
        
        // Use same logic as FUSE file_handles.rs create_processing_item_for_handle()
        let operation = if onedrive_id.starts_with("local_") {
            crate::persistency::processing_item_repository::ChangeOperation::Create  // New folder with temporary ID
        } else {
            crate::persistency::processing_item_repository::ChangeOperation::Update  // Existing folder
        };
        
        let processing_item = crate::persistency::processing_item_repository::ProcessingItem::new_local(
            item.drive_item().clone(), 
            operation
        );
        
        // Store processing item (will get higher ID for correct order)
        let _id = self.processing_repo.store_processing_item(&processing_item).await?;
        
        debug!("📋 Created ProcessingItem for folder: {} (DB ID: {})", 
               item.drive_item().name.as_deref().unwrap_or("unnamed"), _id);
        
        Ok(())
    }

    /// Update item parent and reorder processing item
    async fn update_item_parent_and_reorder(&self, item: &ProcessingItem, new_parent_ino: u64) -> Result<()> {
        // Get the new parent's OneDrive ID
        let new_parent_item = self.drive_item_with_fuse_repo
            .get_drive_item_with_fuse_by_virtual_ino(new_parent_ino)
            .await?
            .unwrap();
        let new_parent_onedrive_id = new_parent_item.drive_item().id.clone();
        
        // Update DriveItem parent reference
        let mut updated_drive_item = item.drive_item.clone();
        if let Some(parent_ref) = updated_drive_item.parent_reference.as_mut() {
            parent_ref.id = new_parent_onedrive_id;
            // Update parent path from new parent
            parent_ref.path = new_parent_item.virtual_path()
                .map(|p| format!("/drive/root:{}", p));
        }

        // Update DriveItemWithFuse if it exists (for Update operations)
        // For Create operations, the item might not exist yet in the database
        if let Ok(Some(mut updated_fuse_item)) = self.drive_item_with_fuse_repo
            .get_drive_item_with_fuse(&item.drive_item.id)
            .await 
        {
            // Update parent inode
            updated_fuse_item.set_parent_ino(new_parent_ino);
            
            // Store updated DriveItemWithFuse
            updated_fuse_item.drive_item = updated_drive_item.clone();
            self.drive_item_with_fuse_repo
                .store_drive_item_with_fuse(&updated_fuse_item)
                .await?;
        }
        
        // Delete current processing item
        if let Some(current_id) = item.id {
            self.processing_repo.delete_processing_item_by_id(current_id).await?;
        }
        
        // Create new processing item with updated data (gets higher ID)
        let new_processing_item = crate::persistency::processing_item_repository::ProcessingItem::new_local(
            updated_drive_item,
            item.change_operation.clone()
        );
        
        let _new_id = self.processing_repo.store_processing_item(&new_processing_item).await?;
        
        info!("🔄 Reordered processing item: {} -> new order", 
              item.drive_item.name.as_deref().unwrap_or("unnamed"));
        
        Ok(())
    }
}
