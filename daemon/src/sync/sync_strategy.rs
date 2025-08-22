use crate::app_state::AppState;
use crate::persistency::drive_item_with_fuse_repository::DriveItemWithFuseRepository;
use crate::persistency::processing_item_repository::{ChangeOperation, ChangeType, ProcessingItem, ProcessingStatus};
use crate::sync::conflicts::{LocalConflict, RemoteConflict};
use anyhow::{Context, Result};
use log::{debug, info, warn};
use std::sync::Arc;

pub struct SyncStrategy {
    app_state: Arc<AppState>,
    drive_item_with_fuse_repo: DriveItemWithFuseRepository,
}

impl SyncStrategy {
    pub fn new(app_state: Arc<AppState>) -> Self {
        let drive_item_with_fuse_repo = app_state.persistency().drive_item_with_fuse_repository();
        Self {
            app_state,
            drive_item_with_fuse_repo,
        }
    }

    pub async fn detect_remote_conflicts(
        &self,
        item: &ProcessingItem,
    ) -> Result<Vec<RemoteConflict>> {
        let mut conflicts = Vec::new();

        // 1. Parent folder state
        if let Some(parent_ref) = &item.drive_item.parent_reference {
            if parent_ref.id != "" {
                if let Ok(Some(parent_item)) = self
                    .drive_item_with_fuse_repo
                    .get_drive_item_with_fuse(&parent_ref.id)
                    .await
                {
                    if parent_item.is_deleted() {
                        conflicts.push(RemoteConflict::ModifyOnParentDelete);
                    }
                } else {
                    // Parent does not exist, might be a conflict if the item is not a create operation
                    if item.change_operation != ChangeOperation::Create {
                        conflicts.push(RemoteConflict::MoveToDeletedParent)
                    }
                }
            }
        }

        // 2. Name collision
        if item.change_operation != ChangeOperation::Delete {
            if let Some(parent_ref) = &item.drive_item.parent_reference {
                if let Ok(siblings) = self
                    .drive_item_with_fuse_repo
                    .get_drive_items_with_fuse_by_parent(&parent_ref.id)
                    .await
                {
                    let item_name = item.drive_item.name.as_deref().unwrap_or("");
                    for sibling in siblings {
                        if sibling.id() != item.drive_item.id
                            && sibling.name().unwrap_or("").eq_ignore_ascii_case(item_name)
                        {
                            conflicts.push(RemoteConflict::RenameOrMoveOnExisting);
                            break;
                        }
                    }
                }
            }
        }

        // 3. Content conflict
        let processing_item_repo = self.app_state.persistency().processing_item_repository();
        if let Ok(Some(local_change)) = processing_item_repo
            .get_pending_processing_item_by_drive_item_id_and_change_type(
                &item.drive_item.id,
                &ChangeType::Local,
            )
            .await
        {
            match (&item.change_operation, &local_change.change_operation) {
                (ChangeOperation::Update { .. }, ChangeOperation::Update { .. }) => {
                    if let Ok(Some(existing_item)) = self.drive_item_with_fuse_repo
                    .get_drive_item_with_fuse(&item.drive_item.id)
                    .await
                {
                    match &existing_item.fuse_metadata.ctag {
                        Some(existing_ctag) => {
                            // We have a stored ctag - check if we need to retrieve remote ctag
                            if item.drive_item.ctag.is_none() {
                                // Delta API didn't provide ctag, we need to fetch it
                                match self.app_state.onedrive_client
                                    .get_item_by_id(&item.drive_item.id)
                                    .await
                                {
                                    Ok(remote_item) => {
                                        if let Some(remote_ctag) = &remote_item.ctag {
                                            if existing_ctag != remote_ctag {
                                                conflicts.push(RemoteConflict::ContentConflict(
                                                    existing_ctag.clone(),
                                                    remote_ctag.clone(),
                                                ));
                                            } else {
                                                // Ctags match - only ETag changed (metadata update)
                                                debug!("Ctags match, ETag change is metadata-only for item: {}", item.drive_item.id);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to retrieve ctag for item {}: {}", item.drive_item.id, e);
                                        // Fall back to ETag-based conflict detection
                                        if let Some(existing_etag) = existing_item.etag() {
                                            if let Some(remote_etag) = &item.drive_item.etag {
                                                if existing_etag != remote_etag {
                                                    conflicts.push(RemoteConflict::ModifyOnModify(
                                                        existing_etag.to_string(),
                                                        remote_etag.clone(),
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }
                            } else {
                                // Delta API provided ctag, use it directly
                                if let Some(remote_ctag) = &item.drive_item.ctag {
                                    if existing_ctag != remote_ctag {
                                        conflicts.push(RemoteConflict::ContentConflict(
                                            existing_ctag.clone(),
                                            remote_ctag.clone(),
                                        ));
                                    } else {
                                        // Ctags match - only ETag changed (metadata update)
                                        debug!("Ctags match, ETag change is metadata-only for item: {}", item.drive_item.id);
                                    }
                                }
                            }
                        }
                        None => {
                            // No stored ctag - this item hasn't been synced yet
                            debug!("No stored ctag for item: {}, treating as new", item.drive_item.id);
                        }
                    }
                }
                    
                }
                (ChangeOperation::Create, ChangeOperation::Create) => {
                    conflicts.push(RemoteConflict::CreateOnCreate(item.drive_item.id.clone()));
                }
                (ChangeOperation::Update { .. }, ChangeOperation::Delete) => {
                    conflicts.push(RemoteConflict::ModifyOnDelete);
                }
                (ChangeOperation::Delete, ChangeOperation::Update { .. }) => {
                    conflicts.push(RemoteConflict::DeleteOnModify);
                }
                (ChangeOperation::Delete, ChangeOperation::Move) => {
                    conflicts.push(RemoteConflict::DeleteOnModify);
                }
                (ChangeOperation::Delete, ChangeOperation::Rename) => {
                    conflicts.push(RemoteConflict::DeleteOnModify);
                }
                (ChangeOperation::Delete, ChangeOperation::Create) => {
                    conflicts.push(RemoteConflict::DeleteOnModify);
                }
                (ChangeOperation::Move { .. }, ChangeOperation::Move { .. }) => {
                    if item.drive_item.parent_reference != local_change.drive_item.parent_reference {
                        conflicts.push(RemoteConflict::MoveOnMove);
                    }
                }
                _ => {}
            }
        }

        
    

        Ok(conflicts)
    }

    pub async fn detect_local_conflicts(
        &self,
        item: &ProcessingItem,
    ) -> Result<Vec<LocalConflict>> {
        let mut conflicts = Vec::new();
        

        // Check for existing item on remote to detect certain conflicts
        let remote_item = self.app_state.persistency().processing_item_repository().get_pending_processing_item_by_drive_item_id_and_change_type(&item.drive_item().id, &ChangeType::Remote).await.context("Failed to get remote item")?;

        match &item.change_operation {
            ChangeOperation::Create => {
                if remote_item.is_some() {
                    conflicts.push(LocalConflict::CreateOnExisting);
                }
            }
            ChangeOperation::Update { .. } => {
                if let Some(remote) = remote_item {
                    if remote.change_operation == ChangeOperation::Delete {
                        conflicts.push(LocalConflict::ModifyOnDeleted);
                    } else if remote.drive_item.etag != item.drive_item.etag {
                        // This assumes e_tag is populated from the local DB state before modification
                        conflicts.push(LocalConflict::ModifyOnModified);
                    }
                }
            }
            ChangeOperation::Delete => {
                if let Some(remote) = remote_item {
                     if remote.drive_item.etag != item.drive_item.etag {
                        conflicts.push(LocalConflict::DeleteOnModified);
                    }
                }
            }
            
            ChangeOperation::Rename { .. } | ChangeOperation::Move { .. } => {
                if let Some(remote) = remote_item {
                    if remote.change_operation == ChangeOperation::Delete {
                        conflicts.push(LocalConflict::RenameOrMoveOfDeleted);
                    }
                }
                if let Some(parent_ref) = &item.drive_item.parent_reference {
                    if let Ok(siblings) = self
                        .drive_item_with_fuse_repo
                        .get_drive_items_with_fuse_by_parent(&parent_ref.id)
                        .await
                    {
                        let item_name = item.drive_item.name.as_deref().unwrap_or("");
                        for sibling in siblings {
                            if sibling.id() != item.drive_item.id && sibling.name().unwrap_or("") == item_name {
                                conflicts.push(LocalConflict::RenameOrMoveToExisting);
                                break;
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        Ok(conflicts)
    }

    /// Auto-resolve specific remote conflicts by restoring parent items from OneDrive
    /// Currently handles: ModifyOnParentDelete and MoveToDeletedParent
    pub async fn auto_resolve_remote_conflicts(
        &self,
        item: &ProcessingItem,
        conflicts: &mut Vec<RemoteConflict>,
    ) -> Result<()> {
        let mut resolved_conflicts = Vec::new();
        
        for (index, conflict) in conflicts.iter().enumerate() {
            match conflict {
                RemoteConflict::ModifyOnParentDelete | RemoteConflict::MoveToDeletedParent => {
                    if let Some(parent_ref) = &item.drive_item.parent_reference {
                        if !parent_ref.id.is_empty() {
                            match self.restore_parent_from_onedrive(&parent_ref.id).await {
                                Ok(()) => {
                                    info!(
                                        "✅ Auto-resolved conflict by restoring parent: {} for item {}",
                                        parent_ref.id, item.drive_item.id
                                    );
                                    resolved_conflicts.push(index);
                                }
                                Err(e) => {
                                    warn!(
                                        "⚠️ Failed to auto-resolve conflict by restoring parent {}: {}",
                                        parent_ref.id, e
                                    );
                                }
                            }
                        }
                    }
                }
                RemoteConflict::MetadataOnlyChange => {
                    // Auto-resolve metadata-only changes (no content conflict)
                    info!(
                        "✅ Auto-resolved metadata-only change for item: {}",
                        item.drive_item.id
                    );
                    resolved_conflicts.push(index);
                }
                _ => {
                    // Other conflicts are not auto-resolvable
                }
            }
        }

        // Remove resolved conflicts (in reverse order to maintain indices)
        for &index in resolved_conflicts.iter().rev() {
            conflicts.remove(index);
        }

        Ok(())
    }

    /// Restore a parent item from OneDrive and mark it as not deleted in local database
    async fn restore_parent_from_onedrive(&self, parent_id: &str) -> Result<()> {
        // 1. Fetch parent DriveItem from OneDrive by ID
        let parent_drive_item = self
            .app_state
            .onedrive()
            .get_item_by_id(parent_id)
            .await
            .context("Failed to fetch parent item from OneDrive")?;

        info!(
            "📥 Fetched parent item from OneDrive: {} ({})",
            parent_drive_item.name.as_deref().unwrap_or("unnamed"),
            parent_id
        );

        // 2. Check if parent exists in local database
        if let Ok(Some(local_parent)) = self
            .drive_item_with_fuse_repo
            .get_drive_item_with_fuse(parent_id)
            .await
        {
            // Parent exists locally but is marked as deleted - restore it
            if local_parent.is_deleted() {
                // Create updated DriveItemWithFuse from the OneDrive data
                let mut restored_parent = self
                    .drive_item_with_fuse_repo
                    .create_from_drive_item(parent_drive_item.clone());

                // Preserve the existing virtual inode
                if let Some(existing_ino) = local_parent.virtual_ino() {
                    restored_parent.set_virtual_ino(existing_ino);
                }

                // Mark as not deleted and set as synced
                restored_parent.set_file_source(crate::persistency::types::FileSource::Remote);
                restored_parent.set_sync_status("synced".to_string());

                // Restore parent inode if this parent has its own parent
                if let Some(grandparent_ref) = &parent_drive_item.parent_reference {
                    if let Ok(Some(grandparent_item)) = self
                        .drive_item_with_fuse_repo
                        .get_drive_item_with_fuse(&grandparent_ref.id)
                        .await
                    {
                        if let Some(grandparent_ino) = grandparent_item.virtual_ino() {
                            restored_parent.set_parent_ino(grandparent_ino);
                        }
                    }
                }

                // Store the restored parent (this will mark it as not deleted)
                self.drive_item_with_fuse_repo
                    .store_drive_item_with_fuse(&restored_parent)
                    .await
                    .context("Failed to restore parent in database")?;

                info!(
                    "🔄 Restored deleted parent in database: {} ({})",
                    parent_drive_item.name.as_deref().unwrap_or("unnamed"),
                    parent_id
                );
            } else {
                // Parent exists and is not deleted - just update metadata
                let mut updated_parent = self
                    .drive_item_with_fuse_repo
                    .create_from_drive_item(parent_drive_item.clone());

                // Preserve the existing virtual inode
                if let Some(existing_ino) = local_parent.virtual_ino() {
                    updated_parent.set_virtual_ino(existing_ino);
                }

                updated_parent.set_file_source(crate::persistency::types::FileSource::Remote);
                updated_parent.set_sync_status("synced".to_string());

                self.drive_item_with_fuse_repo
                    .store_drive_item_with_fuse(&updated_parent)
                    .await
                    .context("Failed to update parent in database")?;

                debug!(
                    "📝 Updated parent metadata: {} ({})",
                    parent_drive_item.name.as_deref().unwrap_or("unnamed"),
                    parent_id
                );
            }
        } else {
            // Parent doesn't exist locally - create it
            let mut new_parent = self
                .drive_item_with_fuse_repo
                .create_from_drive_item(parent_drive_item.clone());

            new_parent.set_file_source(crate::persistency::types::FileSource::Remote);
            new_parent.set_sync_status("synced".to_string());

            // Set parent inode if this parent has its own parent
            if let Some(grandparent_ref) = &parent_drive_item.parent_reference {
                if let Ok(Some(grandparent_item)) = self
                    .drive_item_with_fuse_repo
                    .get_drive_item_with_fuse(&grandparent_ref.id)
                    .await
                {
                    if let Some(grandparent_ino) = grandparent_item.virtual_ino() {
                        new_parent.set_parent_ino(grandparent_ino);
                    }
                }
            }

            self.drive_item_with_fuse_repo
                .store_drive_item_with_fuse(&new_parent)
                .await
                .context("Failed to create parent in database")?;

            info!(
                "📁 Created missing parent in database: {} ({})",
                parent_drive_item.name.as_deref().unwrap_or("unnamed"),
                parent_id
            );
        }

        // 3. Remove any processing errors for this parent
        let processing_repo = self.app_state.persistency().processing_item_repository();
        
        // Get all processing items for this parent and clear their errors
        if let Ok(Some(parent_processing_item)) = processing_repo
            .get_processing_item(parent_id)
            .await
        {
            // Clear error status and validation errors
            if let Some(db_id) = parent_processing_item.id {
                processing_repo
                    .update_status_by_id(db_id, &ProcessingStatus::New)
                    .await
                    .context("Failed to reset parent processing status")?;

                processing_repo
                    .update_validation_errors_by_id(db_id, &[])
                    .await
                    .context("Failed to clear parent validation errors")?;

                processing_repo
                    .update_error_message_by_id(db_id, "")
                    .await
                    .context("Failed to clear parent error message")?;

                debug!(
                    "🧹 Cleared processing errors for restored parent: {}",
                    parent_id
                );
            }
        }

        Ok(())
    }
}
