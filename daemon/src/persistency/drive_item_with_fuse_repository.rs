use crate::onedrive_service::onedrive_models::{DriveItem, ParentReference};
use crate::persistency::types::{DriveItemWithFuse, FuseMetadata, FileSource};
use anyhow::{Context, Result};
use log::debug;
use sqlx::{Pool, Row, Sqlite};
use std::path::PathBuf;

/// Database operations for drive items with Fuse metadata
pub struct DriveItemWithFuseRepository {
    pool: Pool<Sqlite>,
}

impl DriveItemWithFuseRepository {
    /// Create a new drive item with Fuse repository
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    /// Create a DriveItemWithFuse from a DriveItem and automatically compute virtual path
    pub fn create_from_drive_item(&self, drive_item: DriveItem) -> DriveItemWithFuse {
        let mut item_with_fuse = DriveItemWithFuse::from_drive_item(drive_item);
        
        // Automatically compute and set the virtual path
        let virtual_path = item_with_fuse.compute_virtual_path();
        item_with_fuse.set_virtual_path(virtual_path);
      
        
        
        item_with_fuse
    }

    /// Create and store a DriveItemWithFuse, returning the auto-generated inode
    pub async fn create_and_store(&self, drive_item: DriveItem, local_path: Option<PathBuf>) -> Result<u64> {
        let item_with_fuse = self.create_from_drive_item(drive_item);
        self.store_drive_item_with_fuse(&item_with_fuse, local_path).await
    }

    /// Store a drive item with Fuse metadata in the database (virtual_ino auto-generated)
    pub async fn store_drive_item_with_fuse(
        &self,
        item: &DriveItemWithFuse,
        local_path: Option<PathBuf>,
    ) -> Result<u64> {
        let parent_id = item.drive_item.parent_reference.as_ref().map(|p| p.id.clone());
        let parent_path = item.drive_item.parent_reference.as_ref().and_then(|p| p.path.clone());
        let local_path_str = local_path.map(|p| p.to_string_lossy().to_string());

        // Check if item already exists to preserve inode
        let existing_item = self.get_drive_item_with_fuse(&item.drive_item.id).await?;
        
        if let Some(existing) = existing_item {
            // Item exists - UPDATE to preserve inode
            let existing_ino = existing.virtual_ino().unwrap_or(0);
            
            sqlx::query(
                r#"
                UPDATE drive_items_with_fuse SET
                    name = ?, etag = ?, last_modified = ?, created_date = ?, size = ?, is_folder = ?,
                    mime_type = ?, download_url = ?, is_deleted = ?, parent_id = ?, parent_path = ?, local_path = ?,
                    parent_ino = ?, virtual_path = ?, display_path = ?, file_source = ?, sync_status = ?,
                    updated_at = CURRENT_TIMESTAMP
                WHERE onedrive_id = ?
                "#,
            )
            .bind(&item.drive_item.name)
            .bind(&item.drive_item.etag)
            .bind(&item.drive_item.last_modified)
            .bind(&item.drive_item.created_date)
            .bind(item.drive_item.size.map(|s| s as i64))
            .bind(item.drive_item.folder.is_some())
            .bind(item.drive_item.file.as_ref().and_then(|f| f.mime_type.clone()))
            .bind(&item.drive_item.download_url)
            .bind(item.drive_item.deleted.is_some())
            .bind(parent_id)
            .bind(parent_path)
            .bind(local_path_str)
            .bind(item.fuse_metadata.parent_ino.map(|i| i as i64))
            .bind(&item.fuse_metadata.virtual_path)
            .bind(&item.fuse_metadata.display_path)
            .bind(item.fuse_metadata.file_source.map(|s| s.as_str()))
            .bind(&item.fuse_metadata.sync_status)
            .bind(&item.drive_item.id)
            .execute(&self.pool)
            .await?;

            debug!(
                "Updated drive item with Fuse: {} ({}) preserving inode {}",
                item.drive_item.name.as_deref().unwrap_or("unnamed"),
                item.drive_item.id,
                existing_ino
            );
            Ok(existing_ino)
        } else {
            // Item doesn't exist - INSERT new record
            let result = sqlx::query(
                r#"
                INSERT INTO drive_items_with_fuse (
                    onedrive_id, name, etag, last_modified, created_date, size, is_folder,
                    mime_type, download_url, is_deleted, parent_id, parent_path, local_path,
                    parent_ino, virtual_path, display_path, file_source, sync_status
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&item.drive_item.id)
            .bind(&item.drive_item.name)
            .bind(&item.drive_item.etag)
            .bind(&item.drive_item.last_modified)
            .bind(&item.drive_item.created_date)
            .bind(item.drive_item.size.map(|s| s as i64))
            .bind(item.drive_item.folder.is_some())
            .bind(item.drive_item.file.as_ref().and_then(|f| f.mime_type.clone()))
            .bind(&item.drive_item.download_url)
            .bind(item.drive_item.deleted.is_some())
            .bind(parent_id)
            .bind(parent_path)
            .bind(local_path_str)
            .bind(item.fuse_metadata.parent_ino.map(|i| i as i64))
            .bind(&item.fuse_metadata.virtual_path)
            .bind(&item.fuse_metadata.display_path)
            .bind(item.fuse_metadata.file_source.map(|s| s.as_str()))
            .bind(&item.fuse_metadata.sync_status)
            .execute(&self.pool)
            .await?;

            let virtual_ino = result.last_insert_rowid() as u64;

            debug!(
                "Inserted new drive item with Fuse: {} ({}) with inode {}",
                item.drive_item.name.as_deref().unwrap_or("unnamed"),
                item.drive_item.id,
                virtual_ino
            );
            Ok(virtual_ino)
        }
    }

    /// Get a drive item with Fuse metadata by OneDrive ID
    pub async fn get_drive_item_with_fuse(&self, onedrive_id: &str) -> Result<Option<DriveItemWithFuse>> {
        let row = sqlx::query(
            r#"
            SELECT virtual_ino, onedrive_id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path, local_path,
                   parent_ino, virtual_path, display_path, file_source, sync_status
            FROM drive_items_with_fuse WHERE onedrive_id = ?
            "#,
        )
        .bind(onedrive_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let drive_item_with_fuse = self.row_to_drive_item_with_fuse(row).await?;
            Ok(Some(drive_item_with_fuse))
        } else {
            Ok(None)
        }
    }


   /// Get all drive items in upload que
   pub async fn get_drive_items_with_fuse_in_download_queue(&self) -> Result<Vec<DriveItemWithFuse>> {
    let rows = sqlx::query(
        r#"
        SELECT virtual_ino, onedrive_id, name, etag, last_modified, created_date, size, is_folder,
               mime_type, download_url, is_deleted, parent_id, parent_path, local_path,
               parent_ino, virtual_path, display_path, file_source, sync_status
        FROM drive_items_with_fuse WHERE 
        onedrive_id  in 
	    (
        SELECT drive_item_id    FROM download_queue where status is not "completed"
        )      
         ORDER BY name
        "#,
    )
    .fetch_all(&self.pool)
    .await?;

    let mut items = Vec::new();
    for row in rows {
        let item = self.row_to_drive_item_with_fuse(row).await?;
        items.push(item);
    }

    Ok(items)
}


    /// Get all drive items with Fuse metadata
    pub async fn get_all_drive_items_with_fuse(&self) -> Result<Vec<DriveItemWithFuse>> {
        let rows = sqlx::query(
            r#"
            SELECT virtual_ino, onedrive_id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path, local_path,
                   parent_ino, virtual_path, display_path, file_source, sync_status
            FROM drive_items_with_fuse ORDER BY name
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut items = Vec::new();
        for row in rows {
            let item = self.row_to_drive_item_with_fuse(row).await?;
            items.push(item);
        }

        Ok(items)
    }

    /// Get drive items with Fuse metadata by parent path
    pub async fn get_drive_items_with_fuse_by_parent_path(
        &self,
        parent_path: &str,
    ) -> Result<Vec<DriveItemWithFuse>> {
        let rows = if parent_path.eq("/") {
            sqlx::query(
                r#"
                SELECT virtual_ino, onedrive_id, name, etag, last_modified, created_date, size, is_folder,
                       mime_type, download_url, is_deleted, parent_id, parent_path, local_path,
                       parent_ino, virtual_path, display_path, file_source, sync_status
                FROM drive_items_with_fuse where parent_path = '/drive/root:' ORDER BY name
                "#,
            )
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT virtual_ino, onedrive_id, name, etag, last_modified, created_date, size, is_folder,
                       mime_type, download_url, is_deleted, parent_id, parent_path, local_path,
                       parent_ino, virtual_path, display_path, file_source, sync_status
                FROM drive_items_with_fuse where REPLACE(parent_path , '/drive/root:' , '') = ? ORDER BY name
                "#,
            )
            .bind(parent_path)
            .fetch_all(&self.pool)
            .await?
        };

        let mut items = Vec::new();
        for row in rows {
            let item = self.row_to_drive_item_with_fuse(row).await?;
            items.push(item);
        }

        Ok(items)
    }

    /// Get drive items with Fuse metadata by parent ID
    pub async fn get_drive_items_with_fuse_by_parent(&self, parent_id: &str) -> Result<Vec<DriveItemWithFuse>> {
        let rows = sqlx::query(
            r#"
            SELECT virtual_ino, onedrive_id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path, local_path,
                   parent_ino, virtual_path, display_path, file_source, sync_status
            FROM drive_items_with_fuse WHERE parent_id = ? ORDER BY name
            "#,
        )
        .bind(parent_id)
        .fetch_all(&self.pool)
        .await?;

        let mut items = Vec::new();
        for row in rows {
            let item = self.row_to_drive_item_with_fuse(row).await?;
            items.push(item);
        }

        Ok(items)
    }


    /// Get children of a directory by parent inode
    pub async fn get_children_by_parent_ino(&self, parent_ino: u64) -> Result<Vec<DriveItemWithFuse>> {
        let rows = sqlx::query(
            r#"
            SELECT virtual_ino, onedrive_id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path, local_path,
                   parent_ino, virtual_path, display_path, file_source, sync_status
            FROM drive_items_with_fuse WHERE parent_ino = ? ORDER BY name
            "#,
        )
        .bind(parent_ino as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut items = Vec::new();
        for row in rows {
            let item = self.row_to_drive_item_with_fuse(row).await?;
            items.push(item);
        }

        Ok(items)
    }

    /// Get children of a directory by parent inode, paginated
    pub async fn get_children_by_parent_ino_paginated(&self, parent_ino: u64, offset: usize, limit: usize) -> Result<Vec<DriveItemWithFuse>> {
        let rows = sqlx::query(
            r#"
            SELECT virtual_ino, onedrive_id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path, local_path,
                   parent_ino, virtual_path, display_path, file_source, sync_status
            FROM drive_items_with_fuse WHERE parent_ino = ? ORDER BY name LIMIT ? OFFSET ?
            "#,
        )
        .bind(parent_ino as i64)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut items = Vec::new();
        for row in rows {
            let item = self.row_to_drive_item_with_fuse(row).await?;
            items.push(item);
        }

        Ok(items)
    }

    /// Get the next available inode number (for debugging/testing)
    pub async fn get_next_inode(&self) -> Result<u64> {
        let row = sqlx::query("SELECT MAX(virtual_ino) as max_ino FROM drive_items_with_fuse")
            .fetch_optional(&self.pool)
            .await?;

        let next_inode = if let Some(row) = row {
            let max_ino: Option<i64> = row.try_get("max_ino")?;
            (max_ino.unwrap_or(0) + 1) as u64
        } else {
            1
        };

        Ok(next_inode)
    }

    /// Check if an inode exists
    pub async fn inode_exists(&self, virtual_ino: u64) -> Result<bool> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM drive_items_with_fuse WHERE virtual_ino = ?"
        )
        .bind(virtual_ino as i64)
        .fetch_one(&self.pool)
        .await?;

        Ok(count > 0)
    }

    /// Get count of items by file source
    pub async fn get_count_by_source(&self, source: FileSource) -> Result<u64> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM drive_items_with_fuse WHERE file_source = ?"
        )
        .bind(source.as_str())
        .fetch_one(&self.pool)
        .await?;

        Ok(count as u64)
    }

    /// Get all items by file source
    pub async fn get_items_by_source(&self, source: FileSource) -> Result<Vec<DriveItemWithFuse>> {
        let rows = sqlx::query(
            r#"
            SELECT virtual_ino, onedrive_id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path, local_path,
                   parent_ino, virtual_path, display_path, file_source, sync_status
            FROM drive_items_with_fuse WHERE file_source = ? ORDER BY name
            "#,
        )
        .bind(source.as_str())
        .fetch_all(&self.pool)
        .await?;

        let mut items = Vec::new();
        for row in rows {
            let item = self.row_to_drive_item_with_fuse(row).await?;
            items.push(item);
        }

        Ok(items)
    }

    /// Get drive item with Fuse metadata by virtual path
    pub async fn get_drive_item_with_fuse_by_virtual_path(&self, virtual_path: &str) -> Result<Option<DriveItemWithFuse>> {
        let row = sqlx::query(
            r#"
            SELECT virtual_ino, onedrive_id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path, local_path,
                   parent_ino, virtual_path, display_path, file_source, sync_status
            FROM drive_items_with_fuse WHERE virtual_path = ?
            "#,
        )
        .bind(virtual_path)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let drive_item_with_fuse = self.row_to_drive_item_with_fuse(row).await?;
            Ok(Some(drive_item_with_fuse))
        } else {
            Ok(None)
        }
    }

    /// Get drive item with Fuse metadata by virtual inode
    pub async fn get_drive_item_with_fuse_by_virtual_ino(&self, virtual_ino: u64) -> Result<Option<DriveItemWithFuse>> {
        let row = sqlx::query(
            r#"
            SELECT virtual_ino, onedrive_id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path, local_path,
                   parent_ino, virtual_path, display_path, file_source, sync_status
            FROM drive_items_with_fuse WHERE virtual_ino = ?
            "#,
        )
        .bind(virtual_ino as i64)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let drive_item_with_fuse = self.row_to_drive_item_with_fuse(row).await?;
            Ok(Some(drive_item_with_fuse))
        } else {
            Ok(None)
        }
    }

    /// Update Fuse metadata for a drive item
    pub async fn update_fuse_metadata(&self, onedrive_id: &str, metadata: &FuseMetadata) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE drive_items_with_fuse 
            SET parent_ino = ?, virtual_path = ?, display_path = ?, local_path = ?,
                file_source = ?, sync_status = ?, updated_at = CURRENT_TIMESTAMP
            WHERE onedrive_id = ?
            "#,
        )
        .bind(metadata.parent_ino.map(|i| i as i64))
        .bind(&metadata.virtual_path)
        .bind(&metadata.display_path)
        .bind(&metadata.local_path)
        .bind(metadata.file_source.map(|s| s.as_str()))
        .bind(&metadata.sync_status)
        .bind(onedrive_id)
        .execute(&self.pool)
        .await?;

        debug!("Updated Fuse metadata for drive item: {}", onedrive_id);
        Ok(())
    }

    /// Delete a drive item with Fuse metadata by OneDrive ID
    pub async fn delete_drive_item_with_fuse(&self, onedrive_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM drive_items_with_fuse WHERE onedrive_id = ?")
            .bind(onedrive_id)
            .execute(&self.pool)
            .await?;

        debug!("Deleted drive item with Fuse: {}", onedrive_id);
        Ok(())
    }

    /// Update OneDrive ID for a drive item (used when temporary ID is replaced with real OneDrive ID)
    pub async fn update_onedrive_id(&self, old_id: &str, new_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE drive_items_with_fuse 
            SET onedrive_id = ?, updated_at = CURRENT_TIMESTAMP
            WHERE onedrive_id = ?
            "#,
        )
        .bind(new_id)
        .bind(old_id)
        .execute(&self.pool)
        .await?;

        debug!("Updated OneDrive ID: {} -> {}", old_id, new_id);
        Ok(())
    }

    /// Update parent ID for all children of a specific parent (used when parent ID changes)
    pub async fn update_parent_id_for_children(&self, old_parent_id: &str, new_parent_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE drive_items_with_fuse 
            SET parent_id = ?, updated_at = CURRENT_TIMESTAMP
            WHERE parent_id = ?
            "#,
        )
        .bind(new_parent_id)
        .bind(old_parent_id)
        .execute(&self.pool)
        .await?;

        debug!("Updated parent ID for children: {} -> {}", old_parent_id, new_parent_id);
        Ok(())
    }

    /// Get all items that have a specific parent ID
    pub async fn get_items_by_parent_id(&self, parent_id: &str) -> Result<Vec<DriveItemWithFuse>> {
        let rows = sqlx::query(
            r#"
            SELECT virtual_ino, onedrive_id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path, local_path,
                   parent_ino, virtual_path, display_path, file_source, sync_status
            FROM drive_items_with_fuse WHERE parent_id = ?
            "#,
        )
        .bind(parent_id)
        .fetch_all(&self.pool)
        .await?;

        let mut items = Vec::new();
        for row in rows {
            let item = self.row_to_drive_item_with_fuse(row).await?;
            items.push(item);
        }

        Ok(items)
    }

    /// Delete a drive item with Fuse metadata by virtual inode
    pub async fn delete_drive_item_with_fuse_by_ino(&self, virtual_ino: u64) -> Result<()> {
        sqlx::query("DELETE FROM drive_items_with_fuse WHERE virtual_ino = ?")
            .bind(virtual_ino as i64)
            .execute(&self.pool)
            .await?;

        debug!("Deleted drive item with Fuse by inode: {}", virtual_ino);
        Ok(())
    }

    /// Convert database row to DriveItemWithFuse
    async fn row_to_drive_item_with_fuse(&self, row: sqlx::sqlite::SqliteRow) -> Result<DriveItemWithFuse> {
        // Extract DriveItem fields
        let virtual_ino: i64 = row.try_get("virtual_ino")?;
        let onedrive_id: String = row.try_get("onedrive_id")?;
        let name: Option<String> = row.try_get("name")?;
        let etag: Option<String> = row.try_get("etag")?;
        let last_modified: Option<String> = row.try_get("last_modified")?;
        let created_date: Option<String> = row.try_get("created_date")?;
        let size: Option<i64> = row.try_get("size")?;
        let is_folder: bool = row.try_get("is_folder")?;
        let mime_type: Option<String> = row.try_get("mime_type")?;
        let download_url: Option<String> = row.try_get("download_url")?;
        let is_deleted: bool = row.try_get("is_deleted")?;
        let parent_id: Option<String> = row.try_get("parent_id")?;
        let parent_path: Option<String> = row.try_get("parent_path")?;
        

        // Build parent reference if available
        let parent_reference = if let Some(id) = parent_id {
            Some(ParentReference {
                id,
                path: parent_path,
            })
        } else {
            None
        };

        // Build folder/file facets
        let folder = if is_folder {
            Some(crate::onedrive_service::onedrive_models::FolderFacet { child_count: 0 })
        } else {
            None
        };

        let file = if !is_folder {
            Some(crate::onedrive_service::onedrive_models::FileFacet { mime_type })
        } else {
            None
        };

        let deleted = if is_deleted {
            Some(crate::onedrive_service::onedrive_models::DeletedFacet {
                state: "deleted".to_string(),
            })
        } else {
            None
        };

        // Create DriveItem
        let drive_item = DriveItem {
            id: onedrive_id,
            name,
            etag,
            last_modified,
            created_date,
            size: size.map(|s| s as u64),
            folder,
            file,
            download_url,
            deleted,
            parent_reference,
        };

        // Extract Fuse metadata fields
        let parent_ino: Option<i64> = row.try_get("parent_ino")?;
        let virtual_path: Option<String> = row.try_get("virtual_path")?;
        let display_path: Option<String> = row.try_get("display_path")?;
        let local_path: Option<String> = row.try_get("local_path")?;
        let file_source_str: Option<String> = row.try_get("file_source")?;
        let sync_status: Option<String> = row.try_get("sync_status")?;

        // Convert file source string to enum
        let file_source = file_source_str.and_then(|s| match s.as_str() {
            "remote" => Some(FileSource::Remote),
            "local" => Some(FileSource::Local),
            "merged" => Some(FileSource::Merged),
            _ => None,
        });

        // Create FuseMetadata
        let fuse_metadata = FuseMetadata {
            virtual_ino: Some(virtual_ino as u64),
            parent_ino: parent_ino.map(|i| i as u64),
            virtual_path,
            display_path,
            local_path,
            file_source,
            sync_status,
        };

        Ok(DriveItemWithFuse {
            drive_item,
            fuse_metadata,
        })
    }
}


