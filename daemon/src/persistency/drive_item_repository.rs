use crate::onedrive_service::onedrive_models::{DriveItem, ParentReference};
use anyhow::{Context, Result};
use log::debug;
use sqlx::{Pool, Row, Sqlite};
use std::path::PathBuf;

/// Database operations for drive items
pub struct DriveItemRepository {
    pool: Pool<Sqlite>,
}

impl DriveItemRepository {
    /// Create a new drive item repository
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    /// Store a drive item in the database
    pub async fn store_drive_item(
        &self,
        item: &DriveItem,
        local_path: Option<PathBuf>,
    ) -> Result<()> {
        let parent_id = item.parent_reference.as_ref().map(|p| p.id.clone());
        let parent_path = item.parent_reference.as_ref().and_then(|p| p.path.clone());
        let local_path_str = local_path.map(|p| p.to_string_lossy().to_string());

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO drive_items (
                id, name, etag, last_modified, created_date, size, is_folder,
                mime_type, download_url, is_deleted, parent_id, parent_path, local_path
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&item.id)
        .bind(&item.name)
        .bind(&item.etag)
        .bind(&item.last_modified)
        .bind(&item.created_date)
        .bind(item.size.map(|s| s as i64))
        .bind(item.folder.is_some())
        .bind(item.file.as_ref().and_then(|f| f.mime_type.clone()))
        .bind(&item.download_url)
        .bind(item.deleted.is_some())
        .bind(parent_id)
        .bind(parent_path)
        .bind(local_path_str)
        .execute(&self.pool)
        .await?;

        debug!(
            "Stored drive item: {} ({})",
            item.name.as_deref().unwrap_or("unnamed"),
            item.id
        );
        Ok(())
    }

    /// Get a drive item by ID
    pub async fn get_drive_item(&self, id: &str) -> Result<Option<DriveItem>> {
        let row = sqlx::query(
            r#"
            SELECT id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path
            FROM drive_items WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let drive_item = self.row_to_drive_item(row).await?;
            Ok(Some(drive_item))
        } else {
            Ok(None)
        }
    }
    #[allow(dead_code)]
    /// Get all drive items
    pub async fn get_all_drive_items(&self) -> Result<Vec<DriveItem>> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path
            FROM drive_items ORDER BY name
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut items = Vec::new();
        for row in rows {
            let item = self.row_to_drive_item(row).await?;
            items.push(item);
        }

        Ok(items)
    }

    pub async fn get_drive_items_by_parent_path(
        &self,
        parent_path: &str,
    ) -> Result<Vec<DriveItem>> {
        let rows = if parent_path.eq("/") {
            sqlx::query(
                r#"
                SELECT id, name, etag, last_modified, created_date, size, is_folder,
                       mime_type, download_url, is_deleted, parent_id, parent_path
                FROM drive_items where parent_path = '/drive/root:'  ORDER BY name
    
                "#,
            )
            .bind(parent_path)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT id, name, etag, last_modified, created_date, size, is_folder,
                       mime_type, download_url, is_deleted, parent_id, parent_path
                FROM drive_items where REPLACE(parent_path , '/drive/root:' , '') = ? ORDER BY name
    
                "#,
            )
            .bind(parent_path)
            .fetch_all(&self.pool)
            .await?
        };

        let mut items = Vec::new();
        for row in rows {
            let item = self.row_to_drive_item(row).await?;
            items.push(item);
        }

        Ok(items)
    }

    #[allow(dead_code)]
    /// Get drive items by parent ID (for folder contents)
    pub async fn get_drive_items_by_parent(&self, parent_id: &str) -> Result<Vec<DriveItem>> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path
            FROM drive_items WHERE parent_id = ? ORDER BY name
            "#,
        )
        .bind(parent_id)
        .fetch_all(&self.pool)
        .await?;

        let mut items = Vec::new();
        for row in rows {
            let item = self.row_to_drive_item(row).await?;
            items.push(item);
        }

        Ok(items)
    }
    #[allow(dead_code)]
    /// Delete a drive item by ID
    pub async fn delete_drive_item(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM drive_items WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        debug!("Deleted drive item: {}", id);
        Ok(())
    }

    /// Convert database row to DriveItem
    async fn row_to_drive_item(&self, row: sqlx::sqlite::SqliteRow) -> Result<DriveItem> {
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

        Ok(DriveItem {
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
        })
    }
} 