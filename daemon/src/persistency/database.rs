//! Database operations for OneDrive sync
//! 
//! This module provides specific database operations for storing and retrieving
//! OneDrive metadata, sync state, and queue management.

use crate::onedrive_service::onedrive_models::{DriveItem, ParentReference, UserProfile};
use anyhow::Result;
use log::{debug, info, warn};
use sqlx::{Pool, Sqlite, Row};
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
    pub async fn store_drive_item(&self, item: &DriveItem, local_path: Option<PathBuf>) -> Result<()> {
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
        
        debug!("Stored drive item: {} ({})", item.name.as_deref().unwrap_or("unnamed"), item.id);
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
    pub async fn store_sync_state(&self, delta_link: Option<String>, status: &str, error_message: Option<String>) -> Result<()> {
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
        
        info!("Stored sync state: status={}, delta_link={:?}", status, delta_link);
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
    pub async fn add_to_download_queue(&self, drive_item_id: &str, local_path: &PathBuf) -> Result<()> {
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
        
        debug!("Added to download queue: {} -> {}", drive_item_id, local_path.display());
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
        
        warn!("Marked download as failed: {} (retry count: {})", queue_id, retry_count);
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
    pub async fn add_to_upload_queue(&self, local_path: &PathBuf, parent_id: Option<String>, file_name: &str) -> Result<()> {
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
        
        debug!("Added to upload queue: {} -> {}", local_path.display(), file_name);
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
        
        warn!("Marked upload as failed: {} (retry count: {})", queue_id, retry_count);
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
        .bind(profile.business_phones.as_ref().map(|phones| phones.join(",")))
        .bind(&profile.mobile_phone)
        .bind(&profile.office_location)
        .bind(&profile.preferred_language)
        .execute(&self.pool)
        .await?;
        
        info!("Stored user profile for: {}", profile.display_name.as_deref().unwrap_or("Unknown"));
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
            let business_phones = business_phones_str
                .map(|phones_str| phones_str.split(',').map(|s| s.trim().to_string()).collect());
            
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