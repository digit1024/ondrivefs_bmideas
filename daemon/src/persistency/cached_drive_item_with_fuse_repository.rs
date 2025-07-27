use crate::onedrive_service::onedrive_models::{DriveItem, ParentReference};
use crate::persistency::types::{DriveItemWithFuse, FuseMetadata, FileSource};
use crate::persistency::drive_item_with_fuse_repository::DriveItemWithFuseRepository;
use anyhow::{Context, Result};
use log::{debug, info};
use sqlx::{Pool, Row, Sqlite};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use chrono::{DateTime, Utc, Duration};
use tokio::sync::RwLock;

/// Cached wrapper around DriveItemWithFuseRepository with inode-based caching
pub struct CachedDriveItemWithFuseRepository {
    inner: Arc<DriveItemWithFuseRepository>,
    cache: Arc<RwLock<HashMap<u64, (DriveItemWithFuse, DateTime<Utc>)>>>,
    cache_ttl: Duration,
}

impl CachedDriveItemWithFuseRepository {
    /// Create a new cached drive item with Fuse repository
    pub fn new(repo: Arc<DriveItemWithFuseRepository>, cache_ttl: Duration) -> Self {
        Self {
            inner: repo,
            cache: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl,
        }
    }

    /// Create a new cached repository with default TTL (5 minutes)
    pub fn new_with_default_ttl(repo: Arc<DriveItemWithFuseRepository>) -> Self {
        Self::new(repo, Duration::seconds(60))
    }

    // Cache management methods
    async fn get_from_cache(&self, inode: u64) -> Option<DriveItemWithFuse> {
        let cache = self.cache.read().await;
        if let Some((item, timestamp)) = cache.get(&inode) {
            if Utc::now().signed_duration_since(*timestamp) < self.cache_ttl {
                debug!("Cache hit for inode: {}", inode);
                return Some(item.clone());
            }
        }
        None
    }

    async fn set_in_cache(&self, inode: u64, item: DriveItemWithFuse) {
        let mut cache = self.cache.write().await;
        cache.insert(inode, (item, Utc::now()));
        drop(cache);
        debug!("Cached item for inode: {}", inode);
        
        self.clean_old_if_cache_gt_2k().await;
    }
 /// Cleans old entries if Cache reached 2000 entries
     async fn clean_old_if_cache_gt_2k(&self) {
         let stat = self.cache_stats().await;
         if stat.0 > 2000 {
             //remove only where timestamp is older than ttl
             let mut cache = self.cache.write().await;
             cache.retain(|_, (_, timestamp)| Utc::now().signed_duration_since(*timestamp) < self.cache_ttl);
         }
     }

         async fn invalidate_cache(&self, inode: u64) {
         let mut cache = self.cache.write().await;
         if cache.remove(&inode).is_some() {
             debug!("Invalidated cache for inode: {}", inode);
         }
     }

     async fn invalidate_all_cache(&self) {
         let mut cache = self.cache.write().await;
         let count = cache.len();
         cache.clear();
         debug!("Invalidated all cache entries: {}", count);
     }

    /// Get cache statistics
    pub async fn cache_stats(&self) -> (usize, Duration) {
        let cache = self.cache.read().await;
        (cache.len(), self.cache_ttl)
    }

    /// Get access to the inner repository (for compatibility with existing code)
    pub fn inner(&self) -> &Arc<DriveItemWithFuseRepository> {
        &self.inner
    }
}

// Implement all the same methods as DriveItemWithFuseRepository
impl CachedDriveItemWithFuseRepository {
    /// Create a DriveItemWithFuse from a DriveItem and automatically compute virtual path
    pub fn create_from_drive_item(&self, drive_item: DriveItem) -> DriveItemWithFuse {
        self.inner.create_from_drive_item(drive_item)
    }

    /// Create and store a DriveItemWithFuse, returning the auto-generated inode
    pub async fn create_and_store(&self, drive_item: DriveItem) -> Result<u64> {
        self.inner.create_and_store(drive_item).await
    }

    /// Store a drive item with Fuse metadata in the database (virtual_ino auto-generated)
    pub async fn store_drive_item_with_fuse(&self, item: &DriveItemWithFuse) -> Result<u64> {
        let inode = self.inner.store_drive_item_with_fuse(item).await?;
        // Invalidate cache for this specific item to prevent stale data
        if let Some(cached_ino) = item.fuse_metadata.virtual_ino {
            self.invalidate_cache(cached_ino).await;
        }
        Ok(inode)
    }

    /// Get a drive item with Fuse metadata by OneDrive ID
    pub async fn get_drive_item_with_fuse(&self, onedrive_id: &str) -> Result<Option<DriveItemWithFuse>> {
        // First try to get from database
        let item = self.inner.get_drive_item_with_fuse(onedrive_id).await?;
        
        // If found, cache it by inode
        if let Some(ref item) = item {
            if let Some(inode) = item.fuse_metadata.virtual_ino {
                self.set_in_cache(inode, item.clone()).await;
            }
        }
        
        Ok(item)
    }

    /// Get all drive items in upload queue
    pub async fn get_drive_items_with_fuse_in_download_queue(&self) -> Result<Vec<DriveItemWithFuse>> {
        self.inner.get_drive_items_with_fuse_in_download_queue().await
    }

    /// Get all drive items with Fuse metadata
    pub async fn get_all_drive_items_with_fuse(&self) -> Result<Vec<DriveItemWithFuse>> {
        self.inner.get_all_drive_items_with_fuse().await
    }

    /// Get drive items with Fuse metadata by parent path
    pub async fn get_drive_items_with_fuse_by_parent_path(&self, parent_path: &str) -> Result<Vec<DriveItemWithFuse>> {
        let items = self.inner.get_drive_items_with_fuse_by_parent_path(parent_path).await?;
        
        // Cache all items by their inodes
        for item in &items {
            if let Some(inode) = item.fuse_metadata.virtual_ino {
                self.set_in_cache(inode, item.clone()).await;
            }
        }
        
        Ok(items)
    }

    /// Get drive items with Fuse metadata by parent ID
    pub async fn get_drive_items_with_fuse_by_parent(&self, parent_id: &str) -> Result<Vec<DriveItemWithFuse>> {
        let items = self.inner.get_drive_items_with_fuse_by_parent(parent_id).await?;
        
        // Cache all items by their inodes
        for item in &items {
            if let Some(inode) = item.fuse_metadata.virtual_ino {
                self.set_in_cache(inode, item.clone()).await;
            }
        }
        
        Ok(items)
    }

    /// Get children of a directory by parent inode
    pub async fn get_children_by_parent_ino(&self, parent_ino: u64) -> Result<Vec<DriveItemWithFuse>> {
        let items = self.inner.get_children_by_parent_ino(parent_ino).await?;
        
        // Cache all items by their inodes
        for item in &items {
            if let Some(inode) = item.fuse_metadata.virtual_ino {
                self.set_in_cache(inode, item.clone()).await;
            }
        }
        
        Ok(items)
    }

    /// Get children of a directory by parent inode, paginated
    pub async fn get_children_by_parent_ino_paginated(&self, parent_ino: u64, offset: usize, limit: usize) -> Result<Vec<DriveItemWithFuse>> {
        let items = self.inner.get_children_by_parent_ino_paginated(parent_ino, offset, limit).await?;
        
        // Cache all items by their inodes
        for item in &items {
            if let Some(inode) = item.fuse_metadata.virtual_ino {
                self.set_in_cache(inode, item.clone()).await;
            }
        }
        
        Ok(items)
    }

    /// Get the next available inode number (for debugging/testing)
    pub async fn get_next_inode(&self) -> Result<u64> {
        self.inner.get_next_inode().await
    }

    /// Check if an inode exists
    pub async fn inode_exists(&self, virtual_ino: u64) -> Result<bool> {
        self.inner.inode_exists(virtual_ino).await
    }

    /// Get count of items by file source
    pub async fn get_count_by_source(&self, source: FileSource) -> Result<u64> {
        self.inner.get_count_by_source(source).await
    }

    /// Get all items by file source
    pub async fn get_items_by_source(&self, source: FileSource) -> Result<Vec<DriveItemWithFuse>> {
  self.inner.get_items_by_source(source).await
        
  
    }

    /// Get drive item with Fuse metadata by virtual path
    pub async fn get_drive_item_with_fuse_by_virtual_path(&self, virtual_path: &str) -> Result<Option<DriveItemWithFuse>> {
        let item = self.inner.get_drive_item_with_fuse_by_virtual_path(virtual_path).await?;
        
        // If found, cache it by inode
        if let Some(ref item) = item {
            if let Some(inode) = item.fuse_metadata.virtual_ino {
                self.set_in_cache(inode, item.clone()).await;
            }
        }
        
        Ok(item)
    }

    /// Get drive item with Fuse metadata by virtual inode - PRIMARY CACHE TARGET
    pub async fn get_drive_item_with_fuse_by_virtual_ino(&self, virtual_ino: u64) -> Result<Option<DriveItemWithFuse>> {
        // First try cache
        if let Some(cached_item) = self.get_from_cache(virtual_ino).await {
            return Ok(Some(cached_item));
        }
        
        // Cache miss - get from database
        let item = self.inner.get_drive_item_with_fuse_by_virtual_ino(virtual_ino).await?;
        
        // If found, cache it
        if let Some(ref item) = item {
            self.set_in_cache(virtual_ino, item.clone()).await;
        }
        
        Ok(item)
    }

    /// Update Fuse metadata for a drive item
    pub async fn update_fuse_metadata(&self, onedrive_id: &str, metadata: &FuseMetadata) -> Result<()> {
        self.inner.update_fuse_metadata(onedrive_id, metadata).await?;
        // Invalidate cache for the affected inode
        if let Some(inode) = metadata.virtual_ino {
            self.invalidate_cache(inode).await;
        }
        Ok(())
    }

    /// Delete a drive item with Fuse metadata by OneDrive ID
    pub async fn delete_drive_item_with_fuse(&self, onedrive_id: &str) -> Result<()> {
        // Get the item first to know its inode for cache invalidation
        let item = self.inner.get_drive_item_with_fuse(onedrive_id).await?;
        let inode = item.as_ref().and_then(|i| i.fuse_metadata.virtual_ino);
        
        self.inner.delete_drive_item_with_fuse(onedrive_id).await?;
        
        // Invalidate cache for this item
        if let Some(inode) = inode {
            self.invalidate_cache(inode).await;
        }
        
        Ok(())
    }

    /// Update OneDrive ID for a drive item (used when temporary ID is replaced with real OneDrive ID)
    pub async fn update_onedrive_id(&self, old_id: &str, new_id: &str) -> Result<()> {
        self.inner.update_onedrive_id(old_id, new_id).await
    }

    /// Update parent ID for all children of a specific parent (used when parent ID changes)
    pub async fn update_parent_id_for_children(&self, old_parent_id: &str, new_parent_id: &str) -> Result<()> {
        self.inner.update_parent_id_for_children(old_parent_id, new_parent_id).await
        
    }

    /// Get all items that have a specific parent ID
    pub async fn get_items_by_parent_id(&self, parent_id: &str) -> Result<Vec<DriveItemWithFuse>> {
        let items = self.inner.get_items_by_parent_id(parent_id).await?;
        
        // Cache all items by their inodes
        for item in &items {
            if let Some(inode) = item.fuse_metadata.virtual_ino {
                self.set_in_cache(inode, item.clone()).await;
            }
        }
        
        Ok(items)
    }

    /// Delete a drive item with Fuse metadata by virtual inode
    pub async fn delete_drive_item_with_fuse_by_ino(&self, virtual_ino: u64) -> Result<()> {
        self.inner.delete_drive_item_with_fuse_by_ino(virtual_ino).await?;
        // Invalidate cache for this inode
        self.invalidate_cache(virtual_ino).await;
        Ok(())
    }

    /// Get a drive item with Fuse metadata by parent inode and name
    pub async fn get_drive_item_with_fuse_by_parent_ino_and_name(&self, parent_ino: u64, name: &str) -> Result<Option<DriveItemWithFuse>> {
        let item = self.inner.get_drive_item_with_fuse_by_parent_ino_and_name(parent_ino, name).await?;
        
        // If found, cache it by inode
        if let Some(ref item) = item {
            if let Some(inode) = item.fuse_metadata.virtual_ino {
                self.set_in_cache(inode, item.clone()).await;
            }
        }
        
        Ok(item)
    }

    /// Get all files (not folders) by virtual_path prefix (for sync folder logic)
    pub async fn get_files_by_virtual_path_prefix(&self, folder_path: &str) -> Result<Vec<DriveItemWithFuse>> {
         self.inner.get_files_by_virtual_path_prefix(folder_path).await
    }
} 