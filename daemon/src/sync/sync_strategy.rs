use crate::app_state::AppState;
use crate::persistency::drive_item_with_fuse_repository::DriveItemWithFuseRepository;
use crate::persistency::processing_item_repository::{ChangeOperation, ChangeType, ProcessingItem};
use crate::sync::conflicts::{LocalConflict, RemoteConflict};
use anyhow::{Context, Result};
use log::debug;
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
                    conflicts.push(RemoteConflict::ModifyOnModify(
                        item.drive_item.id.clone(),
                        local_change.drive_item.id.clone(),
                    ));
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
}
