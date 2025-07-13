//! Database operations for OneDrive sync
//!
//! This module provides specific database operations for storing and retrieving
//! OneDrive metadata, sync state, and queue management.

use crate::onedrive_service::onedrive_models::{DriveItem, ParentReference, UserProfile};
use anyhow::{Context, Result};
use log::{debug, info, warn};
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

/// Database operations for sync state
pub struct SyncStateRepository {
    pool: Pool<Sqlite>,
}

impl SyncStateRepository {
    /// Create a new sync state repository
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    /// Store sync state
    pub async fn store_sync_state(
        &self,
        delta_link: Option<String>,
        status: &str,
        error_message: Option<String>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO sync_state (delta_link, last_sync_time, sync_status, error_message)
            VALUES (?, CURRENT_TIMESTAMP, ?, ?)
            "#,
        )
        .bind(&delta_link)
        .bind(status)
        .bind(error_message)
        .execute(&self.pool)
        .await?;

        info!(
            "Stored sync state: status={}, delta_link={:?}",
            status, delta_link
        );
        Ok(())
    }

    /// Get the latest sync state
    pub async fn get_latest_sync_state(&self) -> Result<Option<(String, String, Option<String>)>> {
        let row = sqlx::query(
            r#"
            SELECT delta_link, sync_status, error_message
            FROM sync_state ORDER BY id DESC LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let delta_link: String = row.try_get("delta_link")?;
            let sync_status: String = row.try_get("sync_status")?;
            let error_message: Option<String> = row.try_get("error_message")?;
            Ok(Some((delta_link, sync_status, error_message)))
        } else {
            Ok(None)
        }
    }
}

/// Database operations for download queue
pub struct DownloadQueueRepository {
    pool: Pool<Sqlite>,
}

impl DownloadQueueRepository {
    /// Create a new download queue repository
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    /// Add item to download queue
    pub async fn add_to_download_queue(
        &self,
        drive_item_id: &str,
        local_path: &PathBuf,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO download_queue (drive_item_id, local_path, status)
            VALUES (?, ?, 'pending')
            "#,
        )
        .bind(drive_item_id)
        .bind(local_path.to_string_lossy())
        .execute(&self.pool)
        .await?;

        debug!(
            "Added to download queue: {} -> {}",
            drive_item_id,
            local_path.display()
        );
        Ok(())
    }

    /// Get pending download items
    pub async fn get_pending_downloads(&self) -> Result<Vec<(i64, String, PathBuf)>> {
        let rows = sqlx::query(
            r#"
            SELECT id, drive_item_id, local_path
            FROM download_queue 
            WHERE status = 'pending' 
            ORDER BY priority DESC, created_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut items = Vec::new();
        for row in rows {
            let id: i64 = row.try_get("id")?;
            let drive_item_id: String = row.try_get("drive_item_id")?;
            let local_path: String = row.try_get("local_path")?;
            items.push((id, drive_item_id, PathBuf::from(local_path)));
        }

        Ok(items)
    }

    /// Mark download as completed
    pub async fn mark_download_completed(&self, queue_id: i64) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE download_queue 
            SET status = 'completed', updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            "#,
        )
        .bind(queue_id)
        .execute(&self.pool)
        .await?;

        debug!("Marked download as completed: {}", queue_id);
        Ok(())
    }

    /// Mark download as failed
    pub async fn mark_download_failed(&self, queue_id: i64, retry_count: i32) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE download_queue 
            SET status = 'failed', retry_count = ?, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            "#,
        )
        .bind(retry_count)
        .bind(queue_id)
        .execute(&self.pool)
        .await?;

        warn!(
            "Marked download as failed: {} (retry count: {})",
            queue_id, retry_count
        );
        Ok(())
    }
}

/// Database operations for upload queue
pub struct UploadQueueRepository {
    pool: Pool<Sqlite>,
}

impl UploadQueueRepository {
    /// Create a new upload queue repository
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    /// Add item to upload queue
    pub async fn add_to_upload_queue(
        &self,
        local_path: &PathBuf,
        parent_id: Option<String>,
        file_name: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO upload_queue (local_path, parent_id, file_name, status)
            VALUES (?, ?, ?, 'pending')
            "#,
        )
        .bind(local_path.to_string_lossy())
        .bind(parent_id)
        .bind(file_name)
        .execute(&self.pool)
        .await?;

        debug!(
            "Added to upload queue: {} -> {}",
            local_path.display(),
            file_name
        );
        Ok(())
    }

    /// Get pending upload items
    pub async fn get_pending_uploads(&self) -> Result<Vec<(i64, PathBuf, Option<String>, String)>> {
        let rows = sqlx::query(
            r#"
            SELECT id, local_path, parent_id, file_name
            FROM upload_queue 
            WHERE status = 'pending' 
            ORDER BY priority DESC, created_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut items = Vec::new();
        for row in rows {
            let id: i64 = row.try_get("id")?;
            let local_path: String = row.try_get("local_path")?;
            let parent_id: Option<String> = row.try_get("parent_id")?;
            let file_name: String = row.try_get("file_name")?;
            items.push((id, PathBuf::from(local_path), parent_id, file_name));
        }

        Ok(items)
    }

    /// Mark upload as completed
    pub async fn mark_upload_completed(&self, queue_id: i64) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE upload_queue 
            SET status = 'completed', updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            "#,
        )
        .bind(queue_id)
        .execute(&self.pool)
        .await?;

        debug!("Marked upload as completed: {}", queue_id);
        Ok(())
    }

    /// Mark upload as failed
    pub async fn mark_upload_failed(&self, queue_id: i64, retry_count: i32) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE upload_queue 
            SET status = 'failed', retry_count = ?, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            "#,
        )
        .bind(retry_count)
        .bind(queue_id)
        .execute(&self.pool)
        .await?;

        warn!(
            "Marked upload as failed: {} (retry count: {})",
            queue_id, retry_count
        );
        Ok(())
    }
}

/// Database operations for user profile
pub struct ProfileRepository {
    pool: Pool<Sqlite>,
}

impl ProfileRepository {
    /// Create a new profile repository
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    /// Store user profile (always overwrites - only one record)
    pub async fn store_profile(&self, profile: &UserProfile) -> Result<()> {
        // First, clear any existing profile records
        sqlx::query("DELETE FROM user_profiles")
            .execute(&self.pool)
            .await?;

        // Insert the new profile
        sqlx::query(
            r#"
            INSERT INTO user_profiles (
                id, display_name, given_name, surname, mail, user_principal_name,
                job_title, business_phones, mobile_phone, office_location, preferred_language
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&profile.id)
        .bind(&profile.display_name)
        .bind(&profile.given_name)
        .bind(&profile.surname)
        .bind(&profile.mail)
        .bind(&profile.user_principal_name)
        .bind(&profile.job_title)
        .bind(
            profile
                .business_phones
                .as_ref()
                .map(|phones| phones.join(",")),
        )
        .bind(&profile.mobile_phone)
        .bind(&profile.office_location)
        .bind(&profile.preferred_language)
        .execute(&self.pool)
        .await?;

        info!(
            "Stored user profile for: {}",
            profile.display_name.as_deref().unwrap_or("Unknown")
        );
        Ok(())
    }

    /// Get the stored user profile
    pub async fn get_profile(&self) -> Result<Option<UserProfile>> {
        let row = sqlx::query(
            r#"
            SELECT id, display_name, given_name, surname, mail, user_principal_name,
                   job_title, business_phones, mobile_phone, office_location, preferred_language
            FROM user_profiles LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let id: String = row.try_get("id")?;
            let display_name: Option<String> = row.try_get("display_name")?;
            let given_name: Option<String> = row.try_get("given_name")?;
            let surname: Option<String> = row.try_get("surname")?;
            let mail: Option<String> = row.try_get("mail")?;
            let user_principal_name: Option<String> = row.try_get("user_principal_name")?;
            let job_title: Option<String> = row.try_get("job_title")?;
            let business_phones_str: Option<String> = row.try_get("business_phones")?;
            let mobile_phone: Option<String> = row.try_get("mobile_phone")?;
            let office_location: Option<String> = row.try_get("office_location")?;
            let preferred_language: Option<String> = row.try_get("preferred_language")?;

            // Parse business phones from comma-separated string
            let business_phones = business_phones_str.map(|phones_str| {
                phones_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect()
            });

            let profile = UserProfile {
                id,
                display_name,
                given_name,
                surname,
                mail,
                user_principal_name,
                job_title,
                business_phones,
                mobile_phone,
                office_location,
                preferred_language,
            };

            Ok(Some(profile))
        } else {
            Ok(None)
        }
    }

    /// Clear the stored user profile
    pub async fn clear_profile(&self) -> Result<()> {
        sqlx::query("DELETE FROM user_profiles")
            .execute(&self.pool)
            .await?;

        info!("Cleared stored user profile");
        Ok(())
    }
}

/// Status enum for processing items
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

/// Processing item that wraps DriveItem with sync state
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
    /// Create a new processing item from a DriveItem
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

    /// Convert back to DriveItem (losing processing state)
    pub fn into_drive_item(self) -> DriveItem {
        self.drive_item
    }

    /// Get the DriveItem reference
    pub fn drive_item(&self) -> &DriveItem {
        &self.drive_item
    }

    /// Get mutable DriveItem reference
    pub fn drive_item_mut(&mut self) -> &mut DriveItem {
        &mut self.drive_item
    }
}

/// Database operations for processing items
pub struct ProcessingItemRepository {
    pool: Pool<Sqlite>,
}

impl ProcessingItemRepository {
    /// Create a new processing item repository
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    /// Store a processing item
    pub async fn store_processing_item(&self, item: &ProcessingItem) -> Result<()> {
        let parent_id = item
            .drive_item
            .parent_reference
            .as_ref()
            .map(|p| p.id.clone());
        let parent_path = item
            .drive_item
            .parent_reference
            .as_ref()
            .and_then(|p| p.path.clone());
        let local_path_str = item
            .local_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO processing_items (
                drive_item_id, name, etag, last_modified, created_date, size, is_folder,
                mime_type, download_url, is_deleted, parent_id, parent_path, local_path,
                status, error_message, last_status_update, retry_count, priority
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
        .bind(
            item.drive_item
                .file
                .as_ref()
                .and_then(|f| f.mime_type.clone()),
        )
        .bind(&item.drive_item.download_url)
        .bind(item.drive_item.deleted.is_some())
        .bind(parent_id)
        .bind(parent_path)
        .bind(local_path_str)
        .bind(item.status.as_str())
        .bind(&item.error_message)
        .bind(&item.last_status_update)
        .bind(item.retry_count)
        .bind(item.priority)
        .execute(&self.pool)
        .await?;

        debug!(
            "Stored processing item: {} ({}) - {}",
            item.drive_item.name.as_deref().unwrap_or("unnamed"),
            item.drive_item.id,
            item.status.as_str()
        );
        Ok(())
    }

    /// Get a processing item by drive item ID
    pub async fn get_processing_item(&self, drive_item_id: &str) -> Result<Option<ProcessingItem>> {
        let row = sqlx::query(
            r#"
            SELECT drive_item_id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path, local_path,
                   status, error_message, last_status_update, retry_count, priority
            FROM processing_items WHERE drive_item_id = ?
            "#,
        )
        .bind(drive_item_id)
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
                   mime_type, download_url, is_deleted, parent_id, parent_path, local_path,
                   status, error_message, last_status_update, retry_count, priority
            FROM processing_items ORDER BY priority DESC, last_status_update ASC
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
    pub async fn get_processing_items_by_status(
        &self,
        status: ProcessingStatus,
    ) -> Result<Vec<ProcessingItem>> {
        let rows = sqlx::query(
            r#"
            SELECT drive_item_id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path, local_path,
                   status, error_message, last_status_update, retry_count, priority
            FROM processing_items WHERE status = ? ORDER BY priority DESC, last_status_update ASC
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

    /// Get items that need processing (new, conflict, error)
    pub async fn get_items_needing_processing(&self) -> Result<Vec<ProcessingItem>> {
        let rows = sqlx::query(
            r#"
            SELECT drive_item_id, name, etag, last_modified, created_date, size, is_folder,
                   mime_type, download_url, is_deleted, parent_id, parent_path, local_path,
                   status, error_message, last_status_update, retry_count, priority
            FROM processing_items 
            WHERE status IN ('new', 'conflict', 'error') 
            ORDER BY priority DESC, last_status_update ASC
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

    /// Update status of a processing item
    pub async fn update_processing_status(
        &self,
        drive_item_id: &str,
        status: ProcessingStatus,
        error_message: Option<String>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE processing_items 
            SET status = ?, error_message = ?, last_status_update = CURRENT_TIMESTAMP
            WHERE drive_item_id = ?
            "#,
        )
        .bind(status.as_str())
        .bind(error_message)
        .bind(drive_item_id)
        .execute(&self.pool)
        .await?;

        debug!(
            "Updated processing status: {} -> {}",
            drive_item_id,
            status.as_str()
        );
        Ok(())
    }

    /// Increment retry count for a processing item
    pub async fn increment_retry_count(&self, drive_item_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE processing_items 
            SET retry_count = retry_count + 1, last_status_update = CURRENT_TIMESTAMP
            WHERE drive_item_id = ?
            "#,
        )
        .bind(drive_item_id)
        .execute(&self.pool)
        .await?;

        debug!("Incremented retry count for: {}", drive_item_id);
        Ok(())
    }
    pub async fn get_new_items_count(&self) -> Result<i64> {
        let count = sqlx::query("SELECT COUNT(1) as c FROM processing_items WHERE status = 'new'")
            .fetch_one(&self.pool)
            .await?;
        let c: i64 = count.try_get("c")?;
        Ok(c)
    }

    /// Delete a processing item
    pub async fn delete_processing_item(&self, drive_item_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM processing_items WHERE drive_item_id = ?")
            .bind(drive_item_id)
            .execute(&self.pool)
            .await?;

        debug!("Deleted processing item: {}", drive_item_id);
        Ok(())
    }

    /// Convert database row to ProcessingItem
    async fn row_to_processing_item(&self, row: sqlx::sqlite::SqliteRow) -> Result<ProcessingItem> {
        let drive_item_id: String = row.try_get("drive_item_id")?;
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
        let local_path_str: Option<String> = row.try_get("local_path")?;
        let status_str: String = row.try_get("status")?;
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
            id: drive_item_id,
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

        let status = ProcessingStatus::from_str(&status_str).unwrap_or(ProcessingStatus::New);

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

/// Local change status enumeration
#[derive(Debug, Clone, PartialEq)]
pub enum LocalChangeStatus {
    New,
    Implemented,
    Reflected,
    Failed,
}

impl LocalChangeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            LocalChangeStatus::New => "new",
            LocalChangeStatus::Implemented => "implemented",
            LocalChangeStatus::Reflected => "reflected",
            LocalChangeStatus::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "new" => Some(LocalChangeStatus::New),
            "implemented" => Some(LocalChangeStatus::Implemented),
            "reflected" => Some(LocalChangeStatus::Reflected),
            "failed" => Some(LocalChangeStatus::Failed),
            _ => None,
        }
    }
}

/// Local change type enumeration
#[derive(Debug, Clone, PartialEq)]
pub enum LocalChangeType {
    CreateFile,
    CreateFolder,
    Modify,
    Delete,
    Move,
}

impl LocalChangeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            LocalChangeType::CreateFile => "create_file",
            LocalChangeType::CreateFolder => "create_folder",
            LocalChangeType::Modify => "modify",
            LocalChangeType::Delete => "delete",
            LocalChangeType::Move => "move",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "create_file" => Some(LocalChangeType::CreateFile),
            "create_folder" => Some(LocalChangeType::CreateFolder),
            "modify" => Some(LocalChangeType::Modify),
            "delete" => Some(LocalChangeType::Delete),
            "move" => Some(LocalChangeType::Move),
            _ => None,
        }
    }
}

/// Local change item representing a local file system change
#[derive(Debug, Clone)]
pub struct LocalChange {
    pub id: Option<i64>,
    pub temporary_id: String,
    pub onedrive_id: Option<String>,
    pub change_type: LocalChangeType,
    pub virtual_path: String,
    pub old_virtual_path: Option<String>,
    pub parent_id: Option<String>,
    pub file_name: Option<String>,
    pub content_file_id: Option<String>,
    pub base_etag: Option<String>,
    pub status: LocalChangeStatus,
    pub file_hash: Option<String>,
    pub file_size: Option<i64>,
    pub mime_type: Option<String>,
    pub temp_name: Option<String>,
    pub temp_size: Option<i64>,
    pub temp_mime_type: Option<String>,
    pub temp_created_date: Option<String>,
    pub temp_last_modified: Option<String>,
    pub temp_is_folder: Option<bool>,
    pub error_message: Option<String>,
    pub retry_count: i32,
    pub priority: i32,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

impl LocalChange {
    /// Create a new local change
    pub fn new(
        temporary_id: String,
        change_type: LocalChangeType,
        virtual_path: String,
        parent_id: Option<String>,
        file_name: Option<String>,
        content_file_id: Option<String>,
        temp_is_folder: Option<bool>,
    ) -> Self {
        Self {
            id: None,
            temporary_id,
            onedrive_id: None,
            change_type,
            virtual_path,
            old_virtual_path: None,
            parent_id,
            file_name: file_name.clone(),
            content_file_id,
            base_etag: None,
            status: LocalChangeStatus::New,
            file_hash: None,
            file_size: None,
            mime_type: None,
            temp_name: file_name,
            temp_size: None,
            temp_mime_type: None,
            temp_created_date: None,
            temp_last_modified: None,
            temp_is_folder,
            error_message: None,
            retry_count: 0,
            priority: 0,
            created_at: None,
            updated_at: None,
        }
    }
}

/// Database operations for local changes
pub struct LocalChangesRepository {
    pool: Pool<Sqlite>,
}

impl LocalChangesRepository {
    /// Create a new local changes repository
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    /// Store a local change in the database
    pub async fn store_local_change(&self, change: &LocalChange) -> Result<()> {
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO local_changes (
                id, temporary_id, onedrive_id, change_type, virtual_path, old_virtual_path,
                parent_id, file_name, content_file_id, base_etag, status, file_hash,
                file_size, mime_type, temp_name, temp_size, temp_mime_type,
                temp_created_date, temp_last_modified, temp_is_folder, error_message,
                retry_count, priority, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(change.id)
        .bind(&change.temporary_id)
        .bind(&change.onedrive_id)
        .bind(change.change_type.as_str())
        .bind(&change.virtual_path)
        .bind(&change.old_virtual_path)
        .bind(&change.parent_id)
        .bind(&change.file_name)
        .bind(&change.content_file_id)
        .bind(&change.base_etag)
        .bind(change.status.as_str())
        .bind(&change.file_hash)
        .bind(change.file_size)
        .bind(&change.mime_type)
        .bind(&change.temp_name)
        .bind(change.temp_size)
        .bind(&change.temp_mime_type)
        .bind(&change.temp_created_date)
        .bind(&change.temp_last_modified)
        .bind(change.temp_is_folder)
        .bind(&change.error_message)
        .bind(change.retry_count)
        .bind(change.priority)
        .bind(&change.created_at)
        .bind(&change.updated_at)
        .execute(&self.pool)
        .await?;

        debug!(
            "Stored local change: {} ({})",
            change.virtual_path, change.temporary_id
        );
        Ok(())
    }

    /// Get a local change by temporary ID
    pub async fn get_local_change_by_temporary_id(
        &self,
        temporary_id: &str,
    ) -> Result<Option<LocalChange>> {
        let row = sqlx::query(
            r#"
            SELECT id, temporary_id, onedrive_id, change_type, virtual_path, old_virtual_path,
                   parent_id, file_name, content_file_id, base_etag, status, file_hash,
                   file_size, mime_type, temp_name, temp_size, temp_mime_type,
                   temp_created_date, temp_last_modified, temp_is_folder, error_message,
                   retry_count, priority, created_at, updated_at
            FROM local_changes WHERE temporary_id = ?
            "#,
        )
        .bind(temporary_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let local_change = self.row_to_local_change(row).await?;
            Ok(Some(local_change))
        } else {
            Ok(None)
        }
    }

    /// Get a local change by OneDrive ID
    pub async fn get_local_change_by_onedrive_id(
        &self,
        onedrive_id: &str,
    ) -> Result<Option<LocalChange>> {
        let row = sqlx::query(
            r#"
            SELECT id, temporary_id, onedrive_id, change_type, virtual_path, old_virtual_path,
                   parent_id, file_name, content_file_id, base_etag, status, file_hash,
                   file_size, mime_type, temp_name, temp_size, temp_mime_type,
                   temp_created_date, temp_last_modified, temp_is_folder, error_message,
                   retry_count, priority, created_at, updated_at
            FROM local_changes WHERE onedrive_id = ?
            "#,
        )
        .bind(onedrive_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let local_change = self.row_to_local_change(row).await?;
            Ok(Some(local_change))
        } else {
            Ok(None)
        }
    }

    /// Get all local changes by status
    pub async fn get_local_changes_by_status(
        &self,
        status: LocalChangeStatus,
    ) -> Result<Vec<LocalChange>> {
        let rows = sqlx::query(
            r#"
            SELECT id, temporary_id, onedrive_id, change_type, virtual_path, old_virtual_path,
                   parent_id, file_name, content_file_id, base_etag, status, file_hash,
                   file_size, mime_type, temp_name, temp_size, temp_mime_type,
                   temp_created_date, temp_last_modified, temp_is_folder, error_message,
                   retry_count, priority, created_at, updated_at
            FROM local_changes WHERE status = ? ORDER BY priority DESC, created_at ASC
            "#,
        )
        .bind(status.as_str())
        .fetch_all(&self.pool)
        .await?;

        let mut changes = Vec::new();
        for row in rows {
            let change = self.row_to_local_change(row).await?;
            changes.push(change);
        }

        Ok(changes)
    }

    /// Get all local changes that are not reflected (not confirmed by delta API)
    pub async fn get_pending_local_changes(&self) -> Result<Vec<LocalChange>> {
        let rows = sqlx::query(
            r#"
            SELECT id, temporary_id, onedrive_id, change_type, virtual_path, old_virtual_path,
                   parent_id, file_name, content_file_id, base_etag, status, file_hash,
                   file_size, mime_type, temp_name, temp_size, temp_mime_type,
                   temp_created_date, temp_last_modified, temp_is_folder, error_message,
                   retry_count, priority, created_at, updated_at
            FROM local_changes WHERE status != 'reflected' ORDER BY priority DESC, created_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut changes = Vec::new();
        for row in rows {
            let change = self.row_to_local_change(row).await?;
            changes.push(change);
        }

        Ok(changes)
    }

    /// Update the OneDrive ID for a local change
    pub async fn update_onedrive_id(&self, temporary_id: &str, onedrive_id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE local_changes SET onedrive_id = ?, updated_at = CURRENT_TIMESTAMP WHERE temporary_id = ?",
        )
        .bind(onedrive_id)
        .bind(temporary_id)
        .execute(&self.pool)
        .await?;

        debug!(
            "Updated OneDrive ID for local change: {} -> {}",
            temporary_id, onedrive_id
        );
        Ok(())
    }

    /// Update the status of a local change
    pub async fn update_status(&self, temporary_id: &str, status: LocalChangeStatus) -> Result<()> {
        sqlx::query(
            "UPDATE local_changes SET status = ?, updated_at = CURRENT_TIMESTAMP WHERE temporary_id = ?",
        )
        .bind(status.as_str())
        .bind(temporary_id)
        .execute(&self.pool)
        .await?;

        debug!(
            "Updated status for local change: {} -> {}",
            temporary_id,
            status.as_str()
        );
        Ok(())
    }

    /// Update the ETag for a local change
    pub async fn update_etag(&self, temporary_id: &str, etag: &str) -> Result<()> {
        sqlx::query(
            "UPDATE local_changes SET base_etag = ?, updated_at = CURRENT_TIMESTAMP WHERE temporary_id = ?",
        )
        .bind(etag)
        .bind(temporary_id)
        .execute(&self.pool)
        .await?;

        debug!(
            "Updated ETag for local change: {} -> {}",
            temporary_id, etag
        );
        Ok(())
    }

    /// Increment retry count for a local change
    pub async fn increment_retry_count(&self, temporary_id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE local_changes SET retry_count = retry_count + 1, updated_at = CURRENT_TIMESTAMP WHERE temporary_id = ?",
        )
        .bind(temporary_id)
        .execute(&self.pool)
        .await?;

        debug!("Incremented retry count for local change: {}", temporary_id);
        Ok(())
    }

    /// Delete a local change
    pub async fn delete_local_change(&self, temporary_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM local_changes WHERE temporary_id = ?")
            .bind(temporary_id)
            .execute(&self.pool)
            .await?;

        debug!("Deleted local change: {}", temporary_id);
        Ok(())
    }

    /// Generate a new temporary ID
    pub async fn generate_temporary_id(&self) -> Result<String> {
        let count = sqlx::query("SELECT COUNT(1) as c FROM local_changes")
            .fetch_one(&self.pool)
            .await?;
        let c: i64 = count.try_get("c")?;
        Ok(format!("temp_{:03}", c + 1))
    }

    /// Convert database row to LocalChange
    async fn row_to_local_change(&self, row: sqlx::sqlite::SqliteRow) -> Result<LocalChange> {
        let id: Option<i64> = row.try_get("id")?;
        let temporary_id: String = row.try_get("temporary_id")?;
        let onedrive_id: Option<String> = row.try_get("onedrive_id")?;
        let change_type_str: String = row.try_get("change_type")?;
        let virtual_path: String = row.try_get("virtual_path")?;
        let old_virtual_path: Option<String> = row.try_get("old_virtual_path")?;
        let parent_id: Option<String> = row.try_get("parent_id")?;
        let file_name: Option<String> = row.try_get("file_name")?;
        let content_file_id: Option<String> = row.try_get("content_file_id")?;
        let base_etag: Option<String> = row.try_get("base_etag")?;
        let status_str: String = row.try_get("status")?;
        let file_hash: Option<String> = row.try_get("file_hash")?;
        let file_size: Option<i64> = row.try_get("file_size")?;
        let mime_type: Option<String> = row.try_get("mime_type")?;
        let temp_name: Option<String> = row.try_get("temp_name")?;
        let temp_size: Option<i64> = row.try_get("temp_size")?;
        let temp_mime_type: Option<String> = row.try_get("temp_mime_type")?;
        let temp_created_date: Option<String> = row.try_get("temp_created_date")?;
        let temp_last_modified: Option<String> = row.try_get("temp_last_modified")?;
        let temp_is_folder: Option<bool> = row.try_get("temp_is_folder")?;
        let error_message: Option<String> = row.try_get("error_message")?;
        let retry_count: i32 = row.try_get("retry_count")?;
        let priority: i32 = row.try_get("priority")?;
        let created_at: Option<String> = row.try_get("created_at")?;
        let updated_at: Option<String> = row.try_get("updated_at")?;

        let change_type =
            LocalChangeType::from_str(&change_type_str).unwrap_or(LocalChangeType::Modify);
        let status = LocalChangeStatus::from_str(&status_str).unwrap_or(LocalChangeStatus::New);

        Ok(LocalChange {
            id,
            temporary_id,
            onedrive_id,
            change_type,
            virtual_path,
            old_virtual_path,
            parent_id,
            file_name,
            content_file_id,
            base_etag,
            status,
            file_hash,
            file_size,
            mime_type,
            temp_name,
            temp_size,
            temp_mime_type,
            temp_created_date,
            temp_last_modified,
            temp_is_folder,
            error_message,
            retry_count,
            priority,
            created_at,
            updated_at,
        })
    }
}

/// Virtual file item representing a unified view of the filesystem
#[derive(Debug, Clone)]
pub struct VirtualFile {
    pub ino: u64,                        // Inode number
    pub name: String,                    // File name
    pub virtual_path: String,            // Virtual path like "/Documents/file.txt"
    pub parent_ino: Option<u64>,         // Parent inode number
    pub is_folder: bool,                 // Whether this is a folder
    pub size: u64,                       // File size in bytes
    pub mime_type: Option<String>,       // MIME type
    pub created_date: Option<String>,    // Creation date
    pub last_modified: Option<String>,   // Last modification date
    pub content_file_id: Option<String>, // Points to file in downloads/ or changes/
    pub source: FileSource,              // Where this file comes from
    pub sync_status: Option<String>,     // Sync status if applicable
}

/// Source of the file data
#[derive(Debug, Clone, PartialEq)]
pub enum FileSource {
    Remote, // From OneDrive (DriveItems)
    Local,  // From local changes (LocalChanges)
    Merged, // Merged from both sources
}

impl FileSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileSource::Remote => "remote",
            FileSource::Local => "local",
            FileSource::Merged => "merged",
        }
    }
}

/// FUSE repository for unified filesystem view
pub struct FuseRepository {
    pool: Pool<Sqlite>,
    drive_items_repo: DriveItemRepository,
    local_changes_repo: LocalChangesRepository,
}

impl FuseRepository {
    /// Create a new FUSE repository
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self {
            pool: pool.clone(),
            drive_items_repo: DriveItemRepository::new(pool.clone()),
            local_changes_repo: LocalChangesRepository::new(pool),
        }
    }

    /// Get a virtual file by virtual path
    pub async fn get_virtual_file(&self, virtual_path: &str) -> Result<Option<VirtualFile>> {
        // First check if there's a pending local change for this path
        let local_change = self.get_local_change_by_path(virtual_path).await?;

        // Get the remote item if it exists
        let remote_item = self.get_remote_item_by_path(virtual_path).await?;

        // Merge the data
        let virtual_file = self
            .merge_file_data(virtual_path, remote_item, local_change)
            .await?;

        Ok(virtual_file)
    }

    /// List directory contents
    pub async fn list_directory(&self, virtual_path: &str) -> Result<Vec<VirtualFile>> {
        // Get remote items in this directory
        let remote_items = self.get_remote_items_by_parent_path(virtual_path).await?;

        // Get local changes in this directory
        let local_changes = self.get_local_changes_by_parent_path(virtual_path).await?;

        // Merge and deduplicate
        let mut virtual_files = Vec::new();

        // Add remote items
        for item in remote_items {
            if let Some(virtual_file) = self.remote_item_to_virtual_file(&item).await? {
                virtual_files.push(virtual_file);
            }
        }

        // Add local changes, overriding remote items if needed
        for change in &local_changes {
            if let Some(virtual_file) = self.local_change_to_virtual_file(change).await? {
                // Remove any existing remote item with same name
                virtual_files.retain(|f| f.name != virtual_file.name);
                virtual_files.push(virtual_file);
            }
        }

        // Remove items that have pending deletions
        virtual_files.retain(|f| {
            !local_changes.iter().any(|c| {
                c.change_type == LocalChangeType::Delete && c.virtual_path == f.virtual_path
            })
        });

        Ok(virtual_files)
    }

    /// Get file content by virtual path
    pub async fn get_file_content(&self, virtual_path: &str) -> Result<Option<Vec<u8>>> {
        if let Some(virtual_file) = self.get_virtual_file(virtual_path).await? {
            if let Some(content_file_id) = virtual_file.content_file_id {
                // Determine the storage location based on source
                let content_path = match virtual_file.source {
                    FileSource::Remote => {
                        // Content is in downloads/ directory
                        let downloads_dir = self.get_downloads_dir()?;
                        downloads_dir.join(content_file_id)
                    }
                    FileSource::Local => {
                        // Content is in changes/ directory
                        let changes_dir = self.get_changes_dir()?;
                        changes_dir.join(content_file_id)
                    }
                    FileSource::Merged => {
                        // Prefer local content if available, otherwise remote
                        let changes_dir = self.get_changes_dir()?;
                        let local_path = changes_dir.join(&content_file_id);
                        if local_path.exists() {
                            local_path
                        } else {
                            let downloads_dir = self.get_downloads_dir()?;
                            downloads_dir.join(content_file_id)
                        }
                    }
                };

                if content_path.exists() {
                    let content = std::fs::read(&content_path).context(format!(
                        "Failed to read file content: {}",
                        content_path.display()
                    ))?;
                    Ok(Some(content))
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    /// Create a new local change (for file creation/modification)
    pub async fn create_local_change(
        &self,
        change_type: LocalChangeType,
        virtual_path: String,
        parent_path: Option<String>,
        file_name: String,
        content: Option<Vec<u8>>,
        is_folder: bool,
    ) -> Result<()> {
        // Generate temporary ID
        let temporary_id = self.local_changes_repo.generate_temporary_id().await?;

        // Store content if provided
        let content_file_id = if let Some(content_data) = content {
            let changes_dir = self.get_changes_dir()?;
            let content_path = changes_dir.join(&temporary_id);
            std::fs::write(&content_path, content_data).context(format!(
                "Failed to write content: {}",
                content_path.display()
            ))?;
            Some(temporary_id.clone())
        } else {
            None
        };

        // Get parent ID if parent path is provided
        let parent_id = if let Some(parent_path) = parent_path {
            self.get_parent_id_by_path(&parent_path).await?
        } else {
            None
        };

        // Create local change
        let local_change = LocalChange::new(
            temporary_id,
            change_type,
            virtual_path,
            parent_id,
            Some(file_name),
            content_file_id,
            Some(is_folder),
        );

        // Store in database
        self.local_changes_repo
            .store_local_change(&local_change)
            .await?;

        Ok(())
    }

    /// Get parent ID by virtual path
    async fn get_parent_id_by_path(&self, parent_path: &str) -> Result<Option<String>> {
        // Try to find the parent in remote items first
        if let Some(parent_item) = self.get_remote_item_by_path(parent_path).await? {
            return Ok(Some(parent_item.id));
        }

        // Try to find the parent in local changes
        if let Some(parent_change) = self.get_local_change_by_path(parent_path).await? {
            return Ok(parent_change.onedrive_id);
        }

        Ok(None)
    }

    /// Get local change by virtual path
    async fn get_local_change_by_path(&self, virtual_path: &str) -> Result<Option<LocalChange>> {
        let pending_changes = self.local_changes_repo.get_pending_local_changes().await?;

        for change in pending_changes {
            if change.virtual_path == virtual_path {
                return Ok(Some(change));
            }
        }

        Ok(None)
    }

    /// Get remote item by virtual path
    async fn get_remote_item_by_path(&self, virtual_path: &str) -> Result<Option<DriveItem>> {
        let all_items = self.drive_items_repo.get_all_drive_items().await?;

        for item in all_items {
            if let Some(item_path) = self.get_virtual_path_for_item(&item).await? {
                if item_path == virtual_path {
                    return Ok(Some(item));
                }
            }
        }

        Ok(None)
    }

    /// Get remote items by parent path
    async fn get_remote_items_by_parent_path(&self, parent_path: &str) -> Result<Vec<DriveItem>> {
        let items_in_parent = self
            .drive_items_repo
            .get_drive_items_by_parent_path(parent_path)
            .await?;
        // let mut items_in_parent = Vec::new();

        // for item in all_items {
        //     if let Some(item_parent_path) = self.get_parent_path_for_item(&item).await? {
        //         if item_parent_path == parent_path {
        //             items_in_parent.push(item);
        //         }
        //     }
        // }

        Ok(items_in_parent)
    }

    /// Get local changes by parent path
    async fn get_local_changes_by_parent_path(
        &self,
        parent_path: &str,
    ) -> Result<Vec<LocalChange>> {
        let pending_changes = self.local_changes_repo.get_pending_local_changes().await?;
        let mut changes_in_parent = Vec::new();

        for change in pending_changes {
            if let Some(change_parent_path) = self.get_parent_path_for_change(&change).await? {
                if change_parent_path == parent_path {
                    changes_in_parent.push(change);
                }
            }
        }

        Ok(changes_in_parent)
    }

    /// Get virtual path for a DriveItem
    async fn get_virtual_path_for_item(&self, item: &DriveItem) -> Result<Option<String>> {
        if let Some(parent_ref) = &item.parent_reference {
            if let Some(parent_path) = &parent_ref.path {
                // Remove "/drive/root:" prefix to get virtual path
                let virtual_parent_path = parent_path
                    .strip_prefix("/drive/root:")
                    .unwrap_or(parent_path);

                if let Some(name) = &item.name {
                    return Ok(Some(format!("{}/{}", virtual_parent_path, name)));
                }
            }
        }

        // Root level item
        if let Some(name) = &item.name {
            return Ok(Some(format!("/{}", name)));
        }

        Ok(None)
    }

    /// Get parent path for a DriveItem
    async fn get_parent_path_for_item(&self, item: &DriveItem) -> Result<Option<String>> {
        if let Some(parent_ref) = &item.parent_reference {
            if let Some(parent_path) = &parent_ref.path {
                // Remove "/drive/root:" prefix to get virtual path
                let virtual_parent_path = parent_path
                    .strip_prefix("/drive/root:")
                    .unwrap_or(parent_path);
                return Ok(Some(virtual_parent_path.to_string()));
            }
        }

        Ok(Some("/".to_string())) // Root level
    }

    /// Get parent path for a LocalChange
    async fn get_parent_path_for_change(&self, change: &LocalChange) -> Result<Option<String>> {
        if let Some(parent_id) = &change.parent_id {
            // Find the parent item to get its path
            if let Some(parent_item) = self.drive_items_repo.get_drive_item(parent_id).await? {
                return self.get_virtual_path_for_item(&parent_item).await;
            }
        }

        // If no parent ID, assume root level
        Ok(Some("/".to_string()))
    }

    /// Merge file data from remote and local sources
    async fn merge_file_data(
        &self,
        _virtual_path: &str,
        remote_item: Option<DriveItem>,
        local_change: Option<LocalChange>,
    ) -> Result<Option<VirtualFile>> {
        match (remote_item, local_change) {
            (Some(_remote), Some(local)) => {
                // Both exist - local takes precedence
                self.local_change_to_virtual_file(&local).await
            }
            (Some(remote), None) => {
                // Only remote exists
                self.remote_item_to_virtual_file(&remote).await
            }
            (None, Some(local)) => {
                // Only local exists
                self.local_change_to_virtual_file(&local).await
            }
            (None, None) => {
                // Neither exists
                Ok(None)
            }
        }
    }

    /// Convert DriveItem to VirtualFile
    async fn remote_item_to_virtual_file(&self, item: &DriveItem) -> Result<Option<VirtualFile>> {
        if let Some(virtual_path) = self.get_virtual_path_for_item(item).await? {
            let ino = self.generate_inode(&virtual_path);
            let parent_ino = if let Some(parent_path) = self.get_parent_path_for_item(item).await? {
                Some(self.generate_inode(&parent_path))
            } else {
                None
            };

            // Check if file content exists locally
            let content_exists = {
                let downloads_dir = self.get_downloads_dir()?;
                let content_path = downloads_dir.join(&item.id);
                content_path.exists()
            };

            // Determine MIME type based on whether content exists locally
            let mime_type = if content_exists {
                item.file.as_ref().and_then(|f| f.mime_type.clone())
            } else {
                Some("application/onedrivedownload".to_string())
            };

            Ok(Some(VirtualFile {
                ino,
                name: item.name.clone().unwrap_or_else(|| "unnamed".to_string()),
                virtual_path,
                parent_ino,
                is_folder: item.folder.is_some(),
                size: item.size.unwrap_or(0),
                mime_type,
                created_date: item.created_date.clone(),
                last_modified: item.last_modified.clone(),
                content_file_id: Some(item.id.clone()),
                source: FileSource::Remote,
                sync_status: None,
            }))
        } else {
            Ok(None)
        }
    }

    /// Convert LocalChange to VirtualFile
    async fn local_change_to_virtual_file(
        &self,
        change: &LocalChange,
    ) -> Result<Option<VirtualFile>> {
        let ino = self.generate_inode(&change.virtual_path);
        let parent_ino = if let Some(parent_path) = self.get_parent_path_for_change(change).await? {
            Some(self.generate_inode(&parent_path))
        } else {
            None
        };

        Ok(Some(VirtualFile {
            ino,
            name: change
                .file_name
                .clone()
                .unwrap_or_else(|| "unnamed".to_string()),
            virtual_path: change.virtual_path.clone(),
            parent_ino,
            is_folder: change.temp_is_folder.unwrap_or(false),
            size: change.temp_size.unwrap_or(0) as u64,
            mime_type: change.temp_mime_type.clone(),
            created_date: change.temp_created_date.clone(),
            last_modified: change.temp_last_modified.clone(),
            content_file_id: change.content_file_id.clone(),
            source: FileSource::Local,
            sync_status: Some(change.status.as_str().to_string()),
        }))
    }

    /// Generate inode number from virtual path
    fn generate_inode(&self, virtual_path: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        virtual_path.hash(&mut hasher);
        hasher.finish()
    }

    /// Get downloads directory path
    fn get_downloads_dir(&self) -> Result<std::path::PathBuf> {
        let home_dir = std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .map_err(|_| anyhow::anyhow!("HOME environment variable not set"))?;

        Ok(home_dir.join(".local/share/onedrive-sync/downloads"))
    }

    /// Get changes directory path
    fn get_changes_dir(&self) -> Result<std::path::PathBuf> {
        let home_dir = std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .map_err(|_| anyhow::anyhow!("HOME environment variable not set"))?;

        Ok(home_dir.join(".local/share/onedrive-sync/changes"))
    }
}
