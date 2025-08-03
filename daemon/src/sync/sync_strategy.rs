use crate::app_state::AppState;
use crate::onedrive_service::onedrive_models::DriveItem;
use crate::persistency::processing_item_repository::{self, ProcessingItem};
use crate::persistency::drive_item_with_fuse_repository::DriveItemWithFuseRepository;
use crate::sync::conflict_resolution::{ConflictResolver, ConflictResolution};
use crate::sync::strategies::ConflictResolutionFactory;
use crate::persistency::processing_item_repository::UserDecision;
use onedrive_sync_lib::config::ConflictResolutionStrategy;
use std::sync::Arc;
use anyhow::{Context, Result};
use log::{warn, debug};

pub struct SyncStrategy {
    app_state: Arc<AppState>,
    drive_item_with_fuse_repo: DriveItemWithFuseRepository,
}

impl SyncStrategy {
    pub fn new(app_state: Arc<AppState>) -> Self {
        let drive_item_with_fuse_repo = app_state.persistency().drive_item_with_fuse_repository();
        Self { app_state, drive_item_with_fuse_repo }
    }

    pub async fn validate_and_resolve_conflicts(&self, item: &ProcessingItem) -> crate::persistency::processing_item_repository::ValidationResult {
        let mut errors = Vec::new();
        
        // 1. Tree validity check
        if let Err(e) = self.check_tree_validity(item).await {
            errors.push(crate::persistency::processing_item_repository::ValidationError::TreeInvalid(e));
        }
        
        // 2. Name collision check
        if let Err(e) = self.check_name_collision(item).await {
            errors.push(crate::persistency::processing_item_repository::ValidationError::NameCollision(e));
        }
        
        // 3. Content conflict check
        if let Err(e) = self.check_content_conflict(item).await {
            errors.push(crate::persistency::processing_item_repository::ValidationError::ContentConflict(e));
        }
        
        if errors.is_empty() {
            crate::persistency::processing_item_repository::ValidationResult::Valid
        } else {
            // Apply conflict resolution strategy
            let strategy = self.app_state.config().settings.read().await.conflict_resolution_strategy.clone();
            let resolver = ConflictResolutionFactory::create_strategy(&strategy);
            
            // Check if user has already made a decision for manual resolution
            if let Some(user_decision) = &item.user_decision {
                match user_decision {
                    UserDecision::UseRemote => crate::persistency::processing_item_repository::ValidationResult::Resolved(ConflictResolution::UseRemote),
                    UserDecision::UseLocal => crate::persistency::processing_item_repository::ValidationResult::Resolved(ConflictResolution::UseLocal),
                    
                    UserDecision::Skip => crate::persistency::processing_item_repository::ValidationResult::Resolved(ConflictResolution::Skip),
                    UserDecision::Rename { new_name } => {
                        // Handle rename logic
                        crate::persistency::processing_item_repository::ValidationResult::Resolved(ConflictResolution::KeepBoth)
                    }
                }
            } else {
                let resolution = resolver.resolve_conflict(item);
                match resolution {
                    ConflictResolution::Manual => crate::persistency::processing_item_repository::ValidationResult::Invalid(errors),
                    _ => crate::persistency::processing_item_repository::ValidationResult::Resolved(resolution),
                }
            }
        }
    }
    


    /// Check if parent folder exists and is accessible
    async fn check_tree_validity(&self, item: &ProcessingItem) -> Result<(), String> {
        
        //Parent reference for root exists at this point but it's equal to ""
        if let Some(parent_ref)   = &item.drive_item.parent_reference {
            if parent_ref.id == "" {
                //handles root correctly
                return Ok(());
            }
            let drive_item_repo = DriveItemWithFuseRepository::new(self.app_state.persistency().pool().clone());
            
            // Check if parent exists in database
            match drive_item_repo.get_drive_item_with_fuse(&parent_ref.id).await {
                Ok(Some(_)) => Ok(()),
                Ok(None) => Err(format!("Parent folder '{}' does not exist", parent_ref.id)),
                Err(e) => Err(format!("Failed to check parent folder: {}", e)),
            }
        } else {
            // Root item, always valid
            Ok(())
        }
    }

    /// Check for name collisions in the same parent folder
    async fn check_name_collision(&self, item: &ProcessingItem) -> Result<(), String> {
        if item.change_operation == crate::persistency::processing_item_repository::ChangeOperation::Delete {
            return Ok(());
        }   
        if let Some(parent_ref) = &item.drive_item.parent_reference {
            let drive_item_repo = DriveItemWithFuseRepository::new(self.app_state.persistency().pool().clone());
            
            // Get all siblings in the same parent folder
            let siblings = drive_item_repo.get_drive_items_with_fuse_by_parent(&parent_ref.id).await
                .map_err(|e| format!("Failed to get siblings: {}", e))?;
            
            let item_name = item.drive_item.name.as_deref().unwrap_or("");
            
            // Check for name collision (excluding the current item)
            for sibling in siblings {
                if sibling.id() != item.drive_item.id && 
                   sibling.name().unwrap_or("").eq_ignore_ascii_case(item_name) {
                    return Err(format!("File '{}' already exists in this folder", item_name));
                }
            }
        }
        
        Ok(())
    }

    /// Check if item needs smart resolution based on its state
    async fn needs_smart_resolution(&self, item: &ProcessingItem) -> bool {
        // Use smart resolution for complex cases
        match item.change_operation {
            crate::persistency::processing_item_repository::ChangeOperation::Move { .. } => true,
            crate::persistency::processing_item_repository::ChangeOperation::Rename { .. } => true,
            crate::persistency::processing_item_repository::ChangeOperation::Update => {
                // Check if file is downloaded
                if let Ok(Some(fuse_item)) = self.drive_item_with_fuse_repo.get_drive_item_with_fuse(&item.drive_item.id).await {
                    if let Some(ino) = fuse_item.virtual_ino() {
                        let path = self.app_state.file_manager().get_local_path_if_file_exists(ino);
                        let is_downloaded = path.is_some();
                        !is_downloaded // Use smart resolution for not-downloaded files
                    } else {
                        false
                    }
                } else {
                    false
                }
            },
            _ => false
        }
    }

    /// Check for content conflicts between local and remote versions
    async fn check_content_conflict(&self, item: &ProcessingItem) -> Result<(), String> {
        let processing_item_repository = self.app_state.persistency().processing_item_repository();
        let searched_change_type = if (item.change_type == crate::persistency::processing_item_repository::ChangeType::Remote) {
            crate::persistency::processing_item_repository::ChangeType::Local
        } else {
            crate::persistency::processing_item_repository::ChangeType::Remote
        };

        let searched_item = processing_item_repository.get_pending_processing_item_by_drive_item_id_and_change_type(&item.drive_item.id, &searched_change_type).await.map_err(|e| format!("Failed to get pending processing item: {e}"))?;
        match searched_item {
            Some(searched_item) => {
                //If both are deleted, it's ok
                if(item.change_operation == crate::persistency::processing_item_repository::ChangeOperation::Delete && searched_item.change_operation == crate::persistency::processing_item_repository::ChangeOperation::Delete){
                    return Ok(());  
                }else{
                    return Err(format!("File '{}' was modified both locally and remotely", 
                                     item.drive_item.name.as_deref().unwrap_or("unnamed")));
                }
            }
            None => {
                return Ok(());
            }
        }
        
     
    }
} 