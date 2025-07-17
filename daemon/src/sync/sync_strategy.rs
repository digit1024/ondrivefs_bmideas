use crate::app_state::AppState;
use crate::persistency::processing_item_repository::ProcessingItem;
use crate::persistency::drive_item_with_fuse_repository::DriveItemWithFuseRepository;
use crate::sync::conflict_resolution::{ConflictResolver, ConflictResolution};
use crate::sync::strategies::ConflictResolutionFactory;
use crate::persistency::processing_item_repository::UserDecision;
use onedrive_sync_lib::config::ConflictResolutionStrategy;
use std::sync::Arc;
use anyhow::Result;
use log::{warn, debug};

pub struct SyncStrategy {
    app_state: Arc<AppState>,
}

impl SyncStrategy {
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self { app_state }
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
            let strategy = self.app_state.config().settings.conflict_resolution_strategy.clone();
            let resolver = ConflictResolutionFactory::create_strategy(&strategy);
            
            // Check if user has already made a decision for manual resolution
            if let Some(user_decision) = &item.user_decision {
                match user_decision {
                    UserDecision::UseRemote => crate::persistency::processing_item_repository::ValidationResult::Resolved(ConflictResolution::UseRemote),
                    UserDecision::UseLocal => crate::persistency::processing_item_repository::ValidationResult::Resolved(ConflictResolution::UseLocal),
                    UserDecision::Merge => crate::persistency::processing_item_repository::ValidationResult::Resolved(ConflictResolution::Merge),
                    UserDecision::Skip => crate::persistency::processing_item_repository::ValidationResult::Resolved(ConflictResolution::Skip),
                    UserDecision::Rename { new_name } => {
                        // Handle rename logic
                        crate::persistency::processing_item_repository::ValidationResult::Resolved(ConflictResolution::UseLocal)
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
        if let Some(parent_ref) = &item.drive_item.parent_reference {
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
        if let Some(parent_ref) = &item.drive_item.parent_reference {
            let drive_item_repo = DriveItemWithFuseRepository::new(self.app_state.persistency().pool().clone());
            
            // Get all siblings in the same parent folder
            let siblings = drive_item_repo.get_drive_items_with_fuse_by_parent(&parent_ref.id).await
                .map_err(|e| format!("Failed to get siblings: {}", e))?;
            
            let item_name = item.drive_item.name.as_deref().unwrap_or("");
            
            // Check for name collision (excluding the current item)
            for sibling in siblings {
                if sibling.id() != item.drive_item.id && 
                   sibling.name().unwrap_or("") == item_name {
                    return Err(format!("File '{}' already exists in this folder", item_name));
                }
            }
        }
        
        Ok(())
    }

    /// Check for content conflicts between local and remote versions
    async fn check_content_conflict(&self, item: &ProcessingItem) -> Result<(), String> {
        let drive_item_repo = DriveItemWithFuseRepository::new(self.app_state.persistency().pool().clone());
        
        // Get existing item from database
        match drive_item_repo.get_drive_item_with_fuse(&item.drive_item.id).await {
            Ok(Some(existing_item)) => {
                // Check if both local and remote have been modified
                let existing_source = existing_item.file_source();
                let new_source = item.change_type.clone();
                
                if existing_source == Some(crate::persistency::types::FileSource::Local) && 
                   new_source == crate::persistency::processing_item_repository::ChangeType::Remote {
                    return Err(format!("File '{}' was modified both locally and remotely", 
                                     item.drive_item.name.as_deref().unwrap_or("unnamed")));
                }
            }
            Ok(None) => {
                // New item, no conflict
            }
            Err(e) => {
                return Err(format!("Failed to check existing item: {}", e));
            }
        }
        
        Ok(())
    }
} 