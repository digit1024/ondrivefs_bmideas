use crate::onedrive_service::onedrive_models::{DriveItem, ParentReference};
use anyhow::{Context, Result};
use log::{debug, warn};
use sqlx::{Pool, Row, Sqlite};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessingStatus {
    New,
    Processing,
    Conflict,
    Error,
    Done,
}

impl ProcessingStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProcessingStatus::New => "new",
            ProcessingStatus::Processing => "processing",
            ProcessingStatus::Conflict => "conflict",
            ProcessingStatus::Error => "error",
            ProcessingStatus::Done => "done",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "new" => Some(ProcessingStatus::New),
            "processing" => Some(ProcessingStatus::Processing),
            "conflict" => Some(ProcessingStatus::Conflict),
            "error" => Some(ProcessingStatus::Error),
            "done" => Some(ProcessingStatus::Done),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProcessingItem {
    pub drive_item: DriveItem,
    pub status: ProcessingStatus,
    pub local_path: Option<PathBuf>,
    pub error_message: Option<String>,
    pub last_status_update: Option<String>,
    pub retry_count: i32,
    pub priority: i32,
}

impl ProcessingItem {
    pub fn new(drive_item: DriveItem) -> Self {
        Self {
            drive_item,
            status: ProcessingStatus::New,
            local_path: None,
            error_message: None,
            last_status_update: None,
            retry_count: 0,
            priority: 0,
        }
    }

    pub fn into_drive_item(self) -> DriveItem {
        self.drive_item
    }

    pub fn drive_item(&self) -> &DriveItem {
        &self.drive_item
    }

    pub fn drive_item_mut(&mut self) -> &mut DriveItem {
        &mut self.drive_item
    }
}

pub struct ProcessingItemRepository {
    pool: Pool<Sqlite>,
}

impl ProcessingItemRepository {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    /// Store a processing item in the database
    pub async fn store_processing_item(&self, item: &ProcessingItem) -> Result<()> {
        let parent_id = item.drive_item.parent_reference.as_ref().map(|p| p.id.clone());
        let parent_path = item.drive_item.parent_reference.as_ref().and_then(|p| p.path.clone());
        let local_path_str = item.local_path.as_ref().map(|p| p.to_string_lossy().to_string());

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO processing_items (
                drive_item_id, name, etag, last_modified, created_date, size, is_folder,
                mime_type, download_url, is_deleted, parent_id, parent_path,
                status, local_path, error_message, last_status_update, retry_count, priority
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
        .bind(item.status.as_str())
        .bind(local_path_str)
        .bind(&item.error_message)
        .bind(&item.last_status_update)
        .bind(item.retry_count)
        .bind(item.priority)
        .execute(&self.pool)
        .await?;

        debug!(
            "Stored processing item: {} ({}) with status: {:?}",
            item.drive_item.name.as_deref().unwrap_or("unnamed"),
            item.drive_item.id,
            item.status
        );
        Ok(())
    }

    /// Get a processing item by ID
    pub async fn get_processing_item(&self, id: &str) -> Result<Option<ProcessingItem>> {
        let row = sqlx::query(
            r#"
            SELECT drive_item_id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path,
                   status, local_path, error_message, last_status_update, retry_count, priority
            FROM processing_items WHERE drive_item_id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let processing_item = self.row_to_processing_item(row).await?;
            Ok(Some(processing_item))
        } else {
            Ok(None)
        }
    }

    /// Get all processing items
    pub async fn get_all_processing_items(&self) -> Result<Vec<ProcessingItem>> {
        let rows = sqlx::query(
            r#"
            SELECT drive_item_id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path,
                   status, local_path, error_message, last_status_update, retry_count, priority
            FROM processing_items ORDER BY priority DESC, name
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut items = Vec::new();
        for row in rows {
            let item = self.row_to_processing_item(row).await?;
            items.push(item);
        }

        Ok(items)
    }

    /// Get processing items by status
    pub async fn get_processing_items_by_status(&self, status: &ProcessingStatus) -> Result<Vec<ProcessingItem>> {
        let rows = sqlx::query(
            r#"
            SELECT drive_item_id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path,
                   status, local_path, error_message, last_status_update, retry_count, priority
            FROM processing_items WHERE status = ? ORDER BY priority DESC, name
            "#,
        )
        .bind(status.as_str())
        .fetch_all(&self.pool)
        .await?;

        let mut items = Vec::new();
        for row in rows {
            let item = self.row_to_processing_item(row).await?;
            items.push(item);
        }

        Ok(items)
    }

    /// Update the status of a processing item
    pub async fn update_status(&self, id: &str, status: &ProcessingStatus) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE processing_items 
            SET status = ?, last_status_update = datetime('now')
            WHERE drive_item_id = ?
            "#,
        )
        .bind(status.as_str())
        .bind(id)
        .execute(&self.pool)
        .await?;

        debug!("Updated processing item {} status to {:?}", id, status);
        Ok(())
    }

    /// Update the error message of a processing item
    pub async fn update_error_message(&self, id: &str, error_message: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE processing_items 
            SET error_message = ?, last_status_update = datetime('now')
            WHERE drive_item_id = ?
            "#,
        )
        .bind(error_message)
        .bind(id)
        .execute(&self.pool)
        .await?;

        debug!("Updated processing item {} error message: {}", id, error_message);
        Ok(())
    }

    /// Increment retry count for a processing item
    pub async fn increment_retry_count(&self, id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE processing_items 
            SET retry_count = retry_count + 1, last_status_update = datetime('now')
            WHERE drive_item_id = ?
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await?;

        debug!("Incremented retry count for processing item {}", id);
        Ok(())
    }

    /// Update local path for a processing item
    pub async fn update_local_path(&self, id: &str, local_path: &PathBuf) -> Result<()> {
        let local_path_str = local_path.to_string_lossy().to_string();
        
        sqlx::query(
            r#"
            UPDATE processing_items 
            SET local_path = ?, last_status_update = datetime('now')
            WHERE drive_item_id = ?
            "#,
        )
        .bind(local_path_str)
        .bind(id)
        .execute(&self.pool)
        .await?;

        debug!("Updated processing item {} local path: {:?}", id, local_path);
        Ok(())
    }

    /// Delete a processing item by ID
    pub async fn delete_processing_item(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM processing_items WHERE drive_item_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        debug!("Deleted processing item: {}", id);
        Ok(())
    }

    /// Delete all processing items with a specific status
    pub async fn delete_processing_items_by_status(&self, status: &ProcessingStatus) -> Result<()> {
        sqlx::query("DELETE FROM processing_items WHERE status = ?")
            .bind(status.as_str())
            .execute(&self.pool)
            .await?;

        debug!("Deleted all processing items with status: {:?}", status);
        Ok(())
    }

    /// Convert database row to ProcessingItem
    async fn row_to_processing_item(&self, row: sqlx::sqlite::SqliteRow) -> Result<ProcessingItem> {
        let id: String = row.try_get("drive_item_id")?;
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
        let status_str: String = row.try_get("status")?;
        let local_path_str: Option<String> = row.try_get("local_path")?;
        let error_message: Option<String> = row.try_get("error_message")?;
        let last_status_update: Option<String> = row.try_get("last_status_update")?;
        let retry_count: i32 = row.try_get("retry_count")?;
        let priority: i32 = row.try_get("priority")?;

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

        let status = ProcessingStatus::from_str(&status_str)
            .ok_or_else(|| anyhow::anyhow!("Invalid status: {}", status_str))?;

        let local_path = local_path_str.map(PathBuf::from);

        Ok(ProcessingItem {
            drive_item,
            status,
            local_path,
            error_message,
            last_status_update,
            retry_count,
            priority,
        })
    }
} 