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

    /// Store a drive item with Fuse metadata in the database
    pub async fn store_drive_item_with_fuse(
        &self,
        item: &DriveItemWithFuse,
        local_path: Option<PathBuf>,
    ) -> Result<()> {
        let parent_id = item.drive_item.parent_reference.as_ref().map(|p| p.id.clone());
        let parent_path = item.drive_item.parent_reference.as_ref().and_then(|p| p.path.clone());
        let local_path_str = local_path.map(|p| p.to_string_lossy().to_string());

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO drive_items_with_fuse (
                id, name, etag, last_modified, created_date, size, is_folder,
                mime_type, download_url, is_deleted, parent_id, parent_path, local_path,
                virtual_ino, parent_ino, virtual_path, display_path, file_source, sync_status
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        .bind(item.fuse_metadata.virtual_ino.map(|i| i as i64))
        .bind(item.fuse_metadata.parent_ino.map(|i| i as i64))
        .bind(&item.fuse_metadata.virtual_path)
        .bind(&item.fuse_metadata.display_path)
        .bind(item.fuse_metadata.file_source.map(|s| s.as_str()))
        .bind(&item.fuse_metadata.sync_status)
        .execute(&self.pool)
        .await?;

        debug!(
            "Stored drive item with Fuse: {} ({})",
            item.drive_item.name.as_deref().unwrap_or("unnamed"),
            item.drive_item.id
        );
        Ok(())
    }

    /// Get a drive item with Fuse metadata by ID
    pub async fn get_drive_item_with_fuse(&self, id: &str) -> Result<Option<DriveItemWithFuse>> {
        let row = sqlx::query(
            r#"
            SELECT id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path,
                   virtual_ino, parent_ino, virtual_path, display_path, file_source, sync_status
            FROM drive_items_with_fuse WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let drive_item_with_fuse = self.row_to_drive_item_with_fuse(row).await?;
            Ok(Some(drive_item_with_fuse))
        } else {
            Ok(None)
        }
    }

    /// Get all drive items with Fuse metadata
    pub async fn get_all_drive_items_with_fuse(&self) -> Result<Vec<DriveItemWithFuse>> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path,
                   virtual_ino, parent_ino, virtual_path, display_path, file_source, sync_status
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
                SELECT id, name, etag, last_modified, created_date, size, is_folder,
                       mime_type, download_url, is_deleted, parent_id, parent_path,
                       virtual_ino, parent_ino, virtual_path, display_path, file_source, sync_status
                FROM drive_items_with_fuse where parent_path = '/drive/root:' ORDER BY name
                "#,
            )
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT id, name, etag, last_modified, created_date, size, is_folder,
                       mime_type, download_url, is_deleted, parent_id, parent_path,
                       virtual_ino, parent_ino, virtual_path, display_path, file_source, sync_status
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
            SELECT id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path,
                   virtual_ino, parent_ino, virtual_path, display_path, file_source, sync_status
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

    /// Get drive item with Fuse metadata by virtual inode
    pub async fn get_drive_item_with_fuse_by_virtual_ino(&self, virtual_ino: u64) -> Result<Option<DriveItemWithFuse>> {
        let row = sqlx::query(
            r#"
            SELECT id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path,
                   virtual_ino, parent_ino, virtual_path, display_path, file_source, sync_status
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
    pub async fn update_fuse_metadata(&self, id: &str, metadata: &FuseMetadata) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE drive_items_with_fuse 
            SET virtual_ino = ?, parent_ino = ?, virtual_path = ?, display_path = ?, 
                file_source = ?, sync_status = ?, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            "#,
        )
        .bind(metadata.virtual_ino.map(|i| i as i64))
        .bind(metadata.parent_ino.map(|i| i as i64))
        .bind(&metadata.virtual_path)
        .bind(&metadata.display_path)
        .bind(metadata.file_source.map(|s| s.as_str()))
        .bind(&metadata.sync_status)
        .bind(id)
        .execute(&self.pool)
        .await?;

        debug!("Updated Fuse metadata for drive item: {}", id);
        Ok(())
    }

    /// Delete a drive item with Fuse metadata by ID
    pub async fn delete_drive_item_with_fuse(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM drive_items_with_fuse WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        debug!("Deleted drive item with Fuse: {}", id);
        Ok(())
    }

    /// Convert database row to DriveItemWithFuse
    async fn row_to_drive_item_with_fuse(&self, row: sqlx::sqlite::SqliteRow) -> Result<DriveItemWithFuse> {
        // Extract DriveItem fields
        let id: String = row.try_get("id")?;
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
            id,
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
        let virtual_ino: Option<i64> = row.try_get("virtual_ino")?;
        let parent_ino: Option<i64> = row.try_get("parent_ino")?;
        let virtual_path: Option<String> = row.try_get("virtual_path")?;
        let display_path: Option<String> = row.try_get("display_path")?;
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
            virtual_ino: virtual_ino.map(|i| i as u64),
            parent_ino: parent_ino.map(|i| i as u64),
            virtual_path,
            display_path,
            file_source,
            sync_status,
        };

        Ok(DriveItemWithFuse {
            drive_item,
            fuse_metadata,
        })
    }
} 