//! Database operations for FUSE filesystem

use crate::persistency::drive_item_with_fuse_repository::DriveItemWithFuseRepository;
use crate::persistency::cached_drive_item_with_fuse_repository::CachedDriveItemWithFuseRepository;
use crate::persistency::types::{DriveItemWithFuse, FileSource};
use crate::onedrive_service::onedrive_models::DriveItem;
use anyhow::Result;
use log::debug;
use sqlx::types::chrono;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::SystemTime;

/// Database operations manager for the FUSE filesystem
pub struct DatabaseManager {
    drive_item_with_fuse_repo: Arc<CachedDriveItemWithFuseRepository>,
}

impl DatabaseManager {
    pub fn new(drive_item_with_fuse_repo: Arc<CachedDriveItemWithFuseRepository>) -> Self {
        Self { drive_item_with_fuse_repo }
    }

    /// Get DriveItemWithFuse by inode
    pub async fn get_item_by_ino(&self, ino: u64) -> Result<Option<DriveItemWithFuse>> {
        let item = crate::fuse::utils::sync_await(self.drive_item_with_fuse_repo.get_drive_item_with_fuse_by_virtual_ino(ino))?;
        if item.is_some() {
            Ok(item)
        } else {
            if ino == 1 {
                // Access the inner repository for the stub creation
                let inner_repo = self.drive_item_with_fuse_repo.inner();
                Ok(Some(crate::fuse::drive_item_manager::DriveItemManager::create_temp_root_stub(inner_repo)))
            } else {
                Ok(None)
            }
        }
    }

    /// Get DriveItemWithFuse by path
    pub async fn get_item_by_path(&self, path: &str) -> Result<Option<DriveItemWithFuse>> {
        crate::fuse::utils::sync_await(self.drive_item_with_fuse_repo.get_drive_item_with_fuse_by_virtual_path(path))
    }

    /// Get children by parent inode
    pub async fn get_children_by_parent_ino(&self, parent_ino: u64) -> Result<Vec<DriveItemWithFuse>> {
        crate::fuse::utils::sync_await(self.drive_item_with_fuse_repo.get_children_by_parent_ino(parent_ino))
    }

    /// Get children by parent inode, paginated
    pub async fn get_children_by_parent_ino_paginated(&self, parent_ino: u64, offset: usize, limit: usize) -> Result<Vec<DriveItemWithFuse>> {
        crate::fuse::utils::sync_await(self.drive_item_with_fuse_repo.get_children_by_parent_ino_paginated(parent_ino, offset, limit))
    }

    /// Mark database item as modified
    pub async fn mark_db_item_as_modified(&self, ino: u64) -> Result<()> {
        if let Ok(Some(item)) = self.get_item_by_ino(ino).await {
            let mut updated_item = item.clone();
            
            // Update last modified timestamp
            let now = chrono::Utc::now().to_rfc3339();
            updated_item.drive_item_mut().set_last_modified(now);
            
            // Mark as local source
            updated_item.set_file_source(FileSource::Local);
            
            // Store the updated item
            crate::fuse::utils::sync_await(self.drive_item_with_fuse_repo.store_drive_item_with_fuse(&updated_item))?;
            
            debug!("📝 Marked item as modified: ino={}, name={}", 
                   ino, item.name().unwrap_or("unknown"));
        }
        
        Ok(())
    }

    /// Apply local change directly to DriveItemWithFuse table
    pub async fn apply_local_change_to_db_repository(
        &self,
        change_type: &str,
        parent_ino: u64,
        name: &str,
        is_folder: bool,
    ) -> Result<u64> {
        let temporary_id = self.generate_temporary_id();
        
        // Get parent item to extract parent_id and parent_path
        let parent_item = crate::fuse::utils::sync_await(self.get_item_by_ino(parent_ino))?;
        let parent_id = parent_item.as_ref().map(|p| p.id().to_string());
        let parent_path = parent_item.as_ref().and_then(|p| p.virtual_path()).map(|p| format!("/drive/root:{}" , p.to_string()));
        
        // Create a new DriveItem for the local change
        let drive_item = DriveItem {
            id: temporary_id.clone(),
            name: Some(name.to_string()),
            etag: None,
            last_modified: Some(chrono::Utc::now().to_rfc3339()),
            created_date: Some(chrono::Utc::now().to_rfc3339()),
            size: Some(0),
            folder: if is_folder { 
                Some(crate::onedrive_service::onedrive_models::FolderFacet { child_count: 0 }) 
            } else {
                None 
            },
            file: if !is_folder { 
                Some(crate::onedrive_service::onedrive_models::FileFacet { mime_type: None }) 
            } else { 
                None 
            },
            download_url: None,
            deleted: None,
            parent_reference: parent_id.as_ref().map(|id| crate::onedrive_service::onedrive_models::ParentReference {
                id: id.clone(),
                path: parent_path.clone(),
            }),
        };

        let mut item_with_fuse = self.drive_item_with_fuse_repo.create_from_drive_item(drive_item.clone());
        item_with_fuse.set_parent_ino(parent_ino);
        item_with_fuse.set_file_source(FileSource::Local);
        item_with_fuse.set_sync_status("local_change".to_string());

        // Store the item and get the inode
        let inode = crate::fuse::utils::sync_await(self.drive_item_with_fuse_repo.store_drive_item_with_fuse(&item_with_fuse))?;
        
        debug!("📝 Applied local change: type={}, parent_ino={}, name={}, ino={}", 
               change_type, parent_ino, name, inode);
        
        Ok(inode)
    }

    /// Generate a unique temporary ID for local changes
    fn generate_temporary_id(&self) -> String {
        let mut hasher = DefaultHasher::new();
        SystemTime::now().hash(&mut hasher);
        format!("local_{:x}", hasher.finish())
    }
}

use std::sync::Arc; 