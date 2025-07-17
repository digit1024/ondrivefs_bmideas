use crate::app_state::AppState;
use crate::persistency::processing_item_repository::{ProcessingItem, ProcessingStatus, ChangeType, ChangeOperation, ValidationResult};
use crate::sync::sync_strategy::SyncStrategy;
use crate::sync::conflict_resolution::ConflictResolution;
use std::sync::Arc;
use anyhow::Result;
use log::{info, warn, error};

pub struct SyncProcessor {
    strategy: SyncStrategy,
    app_state: Arc<AppState>,
}

impl SyncProcessor {
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self {
            strategy: SyncStrategy::new(app_state.clone()),
            app_state,
        }
    }

    /// Process all items with priority: Remote first, then Local
    pub async fn process_all_items(&self) -> Result<()> {
        let processing_repo = self.app_state.persistency().processing_item_repository();

        // 1. Process Remote changes first
        info!("🔄 Processing remote changes...");
        let remote_items = processing_repo.get_unprocessed_items_by_change_type(&ChangeType::Remote).await?;
        for item in remote_items {
            if let Err(e) = self.process_single_item(&item).await {
                error!("❌ Failed to process remote item: {}", e);
            }
        }

        // 2. Process Local changes after remote changes are handled
        info!("🔄 Processing local changes...");
        let local_items = processing_repo.get_unprocessed_items_by_change_type(&ChangeType::Local).await?;
        for item in local_items {
            if let Err(e) = self.process_single_item(&item).await {
                error!("❌ Failed to process local item: {}", e);
            }
        }

        Ok(())
    }

    /// Process a single item with validation and conflict resolution
    async fn process_single_item(&self, item: &ProcessingItem) -> Result<()> {
        let processing_repo = self.app_state.persistency().processing_item_repository();
        
        // Validate the item
        let validation_result = self.strategy.validate_and_resolve_conflicts(item).await;
        
        match validation_result {
            ValidationResult::Valid => {
                // Mark as validated and ready for processing
                processing_repo.update_status(&item.drive_item.id, &ProcessingStatus::Validated).await?;
                
                // Process the item based on its change type and operation
                match item.change_type {
                    ChangeType::Remote => self.process_remote_item(item).await?,
                    ChangeType::Local => self.process_local_item(item).await?,
                }
            }
            ValidationResult::Invalid(errors) => {
                // Mark as conflicted with error details
                processing_repo.update_status(&item.drive_item.id, &ProcessingStatus::Conflicted).await?;
                
                let error_strings: Vec<String> = errors.iter()
                    .map(|e| e.human_readable())
                    .collect();
                processing_repo.update_validation_errors(&item.drive_item.id, &error_strings).await?;
                
                // Log human-readable errors
                for error in &errors {
                    warn!("❌ Validation error for {}: {}", 
                          item.drive_item.name.as_deref().unwrap_or("unnamed"),
                          error.human_readable());
                }
            }
            ValidationResult::Resolved(resolution) => {
                // Apply the resolution
                match resolution {
                    ConflictResolution::UseRemote => {
                        info!("✅ Using remote version for: {}", 
                              item.drive_item.name.as_deref().unwrap_or("unnamed"));
                        self.apply_remote_resolution(item).await?;
                    }
                    ConflictResolution::UseLocal => {
                        info!("✅ Using local version for: {}", 
                              item.drive_item.name.as_deref().unwrap_or("unnamed"));
                        self.apply_local_resolution(item).await?;
                    }
                    ConflictResolution::Skip => {
                        info!("⏭️ Skipping item: {}", 
                              item.drive_item.name.as_deref().unwrap_or("unnamed"));
                        processing_repo.update_status(&item.drive_item.id, &ProcessingStatus::Cancelled).await?;
                    }
                    ConflictResolution::Merge => {
                        info!("�� Merging item: {}", 
                              item.drive_item.name.as_deref().unwrap_or("unnamed"));
                        self.apply_merge_resolution(item).await?;
                    }
                    ConflictResolution::Manual => {
                        // This should not happen with automatic resolution
                        warn!("⚠️ Manual resolution requested but not implemented");
                    }
                }
            }
        }
        
        Ok(())
    }

    /// Process a remote item (download, update database, etc.)
    async fn process_remote_item(&self, item: &ProcessingItem) -> Result<()> {
        let processing_repo = self.app_state.persistency().processing_item_repository();
        
        processing_repo.update_status(&item.drive_item.id, &ProcessingStatus::Processing).await?;
        
        match item.change_operation {
            ChangeOperation::Create => {
                // Handle remote file creation
                self.handle_remote_create(item).await?;
            }
            ChangeOperation::Update => {
                // Handle remote file update
                self.handle_remote_update(item).await?;
            }
            ChangeOperation::Delete => {
                // Handle remote file deletion
                self.handle_remote_delete(item).await?;
            }
            ChangeOperation::Move { .. } => {
                // Handle remote file move
                self.handle_remote_move(item).await?;
            }
            ChangeOperation::Rename { .. } => {
                // Handle remote file rename
                self.handle_remote_rename(item).await?;
            }
        }
        
        processing_repo.update_status(&item.drive_item.id, &ProcessingStatus::Done).await?;
        Ok(())
    }

    /// Process a local item (upload to OneDrive, etc.)
    async fn process_local_item(&self, item: &ProcessingItem) -> Result<()> {
        let processing_repo = self.app_state.persistency().processing_item_repository();
        
        processing_repo.update_status(&item.drive_item.id, &ProcessingStatus::Processing).await?;
        
        match item.change_operation {
            ChangeOperation::Create => {
                // Handle local file creation (upload to OneDrive)
                self.handle_local_create(item).await?;
            }
            ChangeOperation::Update => {
                // Handle local file update (upload to OneDrive)
                self.handle_local_update(item).await?;
            }
            ChangeOperation::Delete => {
                // Handle local file deletion (delete from OneDrive)
                self.handle_local_delete(item).await?;
            }
            ChangeOperation::Move { .. } => {
                // Handle local file move
                self.handle_local_move(item).await?;
            }
            ChangeOperation::Rename { .. } => {
                // Handle local file rename
                self.handle_local_rename(item).await?;
            }
        }
        
        processing_repo.update_status(&item.drive_item.id, &ProcessingStatus::Done).await?;
        Ok(())
    }

    // Remote operation handlers
    async fn handle_remote_create(&self, item: &ProcessingItem) -> Result<()> {
        info!("📥 Processing remote create: {}", item.drive_item.name.as_deref().unwrap_or("unnamed"));
        // TODO: Implement remote file creation logic
        Ok(())
    }

    async fn handle_remote_update(&self, item: &ProcessingItem) -> Result<()> {
        info!("📝 Processing remote update: {}", item.drive_item.name.as_deref().unwrap_or("unnamed"));
        // TODO: Implement remote file update logic
        Ok(())
    }

    async fn handle_remote_delete(&self, item: &ProcessingItem) -> Result<()> {
        info!("🗑️ Processing remote delete: {}", item.drive_item.name.as_deref().unwrap_or("unnamed"));
        // TODO: Implement remote file deletion logic
        Ok(())
    }

    async fn handle_remote_move(&self, item: &ProcessingItem) -> Result<()> {
        info!("📁 Processing remote move: {}", item.drive_item.name.as_deref().unwrap_or("unnamed"));
        // TODO: Implement remote file move logic
        Ok(())
    }

    async fn handle_remote_rename(&self, item: &ProcessingItem) -> Result<()> {
        info!("🏷️ Processing remote rename: {}", item.drive_item.name.as_deref().unwrap_or("unnamed"));
        // TODO: Implement remote file rename logic
        Ok(())
    }

    // Local operation handlers
    async fn handle_local_create(&self, item: &ProcessingItem) -> Result<()> {
        info!("📤 Processing local create: {}", item.drive_item.name.as_deref().unwrap_or("unnamed"));
        // TODO: Implement local file creation upload logic
        Ok(())
    }

    async fn handle_local_update(&self, item: &ProcessingItem) -> Result<()> {
        info!("📤 Processing local update: {}", item.drive_item.name.as_deref().unwrap_or("unnamed"));
        // TODO: Implement local file update upload logic
        Ok(())
    }

    async fn handle_local_delete(&self, item: &ProcessingItem) -> Result<()> {
        info!("🗑️ Processing local delete: {}", item.drive_item.name.as_deref().unwrap_or("unnamed"));
        // TODO: Implement local file deletion logic
        Ok(())
    }

    async fn handle_local_move(&self, item: &ProcessingItem) -> Result<()> {
        info!("📁 Processing local move: {}", item.drive_item.name.as_deref().unwrap_or("unnamed"));
        // TODO: Implement local file move logic
        Ok(())
    }

    async fn handle_local_rename(&self, item: &ProcessingItem) -> Result<()> {
        info!("🏷️ Processing local rename: {}", item.drive_item.name.as_deref().unwrap_or("unnamed"));
        // TODO: Implement local file rename logic
        Ok(())
    }

    // Conflict resolution handlers
    async fn apply_remote_resolution(&self, item: &ProcessingItem) -> Result<()> {
        // TODO: Implement remote resolution logic
        Ok(())
    }

    async fn apply_local_resolution(&self, item: &ProcessingItem) -> Result<()> {
        // TODO: Implement local resolution logic
        Ok(())
    }

    async fn apply_merge_resolution(&self, item: &ProcessingItem) -> Result<()> {
        // TODO: Implement merge resolution logic
        Ok(())
    }
} 