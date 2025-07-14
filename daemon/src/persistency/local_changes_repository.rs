//! LocalChangesRepository: Handles local_changes table operations 
use anyhow::{Context, Result, anyhow};
use log::{debug, error};
use sqlx::{Pool, Row, Sqlite};
use std::path::PathBuf;

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

#[derive(Debug, Clone, PartialEq)]
pub enum LocalChangeType {
    CreateFile,
    CreateFolder,
    Modify,
    Delete,
    Move,
    Rename,
}

impl LocalChangeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            LocalChangeType::CreateFile => "create_file",
            LocalChangeType::CreateFolder => "create_folder",
            LocalChangeType::Modify => "modify",
            LocalChangeType::Delete => "delete",
            LocalChangeType::Move => "move",
            LocalChangeType::Rename => "rename",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "create_file" => Some(LocalChangeType::CreateFile),
            "create_folder" => Some(LocalChangeType::CreateFolder),
            "modify" => Some(LocalChangeType::Modify),
            "delete" => Some(LocalChangeType::Delete),
            "move" => Some(LocalChangeType::Move),
            "rename" => Some(LocalChangeType::Rename),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LocalChange {
    pub id: Option<i64>,
    pub temporary_id: String,
    pub onedrive_id: Option<String>,
    pub change_type: LocalChangeType,
    pub status: LocalChangeStatus,
    
    // For CREATE operations
    pub parent_id: Option<String>,
    pub file_name: Option<String>,
    
    // For MOVE operations  
    pub old_inode: Option<i64>,
    pub new_inode: Option<i64>,
    
    // For RENAME operations
    pub old_name: Option<String>,
    pub new_name: Option<String>,
    
    // For UPDATE operations
    pub old_etag: Option<String>,
    pub new_etag: Option<String>,
    
    // File metadata
    pub file_size: Option<i64>,
    pub mime_type: Option<String>,
    pub temp_created_date: Option<String>,
    pub temp_last_modified: Option<String>,
    pub temp_is_folder: Option<bool>,
    
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

impl LocalChange {
    // Constructor for backward compatibility
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
            status: LocalChangeStatus::New,
            parent_id,
            file_name,
            old_inode: None,
            new_inode: None,
            old_name: None,
            new_name: None,
            old_etag: None,
            new_etag: None,
            file_size: None,
            mime_type: None,
            temp_created_date: None,
            temp_last_modified: None,
            temp_is_folder,
            created_at: None,
            updated_at: None,
        }
    }

    // Constructor for create operations
    pub fn new_create(
        temporary_id: String,
        change_type: LocalChangeType,
        parent_id: String,
        file_name: String,
        temp_is_folder: bool,
    ) -> Self {
        Self::new_create_with_attrs(
            temporary_id,
            change_type,
            parent_id,
            file_name,
            temp_is_folder,
        )
    }

    // Constructor for create operations with attributes
    pub fn new_create_with_attrs(
        temporary_id: String,
        change_type: LocalChangeType,
        parent_id: String,
        file_name: String,
        temp_is_folder: bool,
    ) -> Self {
        Self {
            id: None,
            temporary_id,
            onedrive_id: None,
            change_type,
            status: LocalChangeStatus::New,
            parent_id: Some(parent_id),
            file_name: Some(file_name),
            old_inode: None,
            new_inode: None,
            old_name: None,
            new_name: None,
            old_etag: None,
            new_etag: None,
            file_size: None,
            mime_type: None,
            temp_created_date: None,
            temp_last_modified: None,
            temp_is_folder: Some(temp_is_folder),
            created_at: None,
            updated_at: None,
        }
    }

    // Constructor for move operations
    pub fn new_move(
        temporary_id: String,
        onedrive_id: String,
        old_inode: i64,
        new_inode: i64,
    ) -> Self {
        Self {
            id: None,
            temporary_id,
            onedrive_id: Some(onedrive_id),
            change_type: LocalChangeType::Move,
            status: LocalChangeStatus::New,
            parent_id: None,
            file_name: None,
            old_inode: Some(old_inode),
            new_inode: Some(new_inode),
            old_name: None,
            new_name: None,
            old_etag: None,
            new_etag: None,
            file_size: None,
            mime_type: None,
            temp_created_date: None,
            temp_last_modified: None,
            temp_is_folder: None,
            created_at: None,
            updated_at: None,
        }
    }

    // Constructor for rename operations
    pub fn new_rename(
        temporary_id: String,
        onedrive_id: String,
        old_name: String,
        new_name: String,
    ) -> Self {
        Self {
            id: None,
            temporary_id,
            onedrive_id: Some(onedrive_id),
            change_type: LocalChangeType::Rename,
            status: LocalChangeStatus::New,
            parent_id: None,
            file_name: None,
            old_inode: None,
            new_inode: None,
            old_name: Some(old_name),
            new_name: Some(new_name),
            old_etag: None,
            new_etag: None,
            file_size: None,
            mime_type: None,
            temp_created_date: None,
            temp_last_modified: None,
            temp_is_folder: None,
            created_at: None,
            updated_at: None,
        }
    }

    // Constructor for modify operations
    pub fn new_modify(
        temporary_id: String,
        onedrive_id: String,
        old_etag: String,
        new_etag: String,
    ) -> Self {
        Self {
            id: None,
            temporary_id,
            onedrive_id: Some(onedrive_id),
            change_type: LocalChangeType::Modify,
            status: LocalChangeStatus::New,
            parent_id: None,
            file_name: None,
            old_inode: None,
            new_inode: None,
            old_name: None,
            new_name: None,
            old_etag: Some(old_etag),
            new_etag: Some(new_etag),
            file_size: None,
            mime_type: None,
            temp_created_date: None,
            temp_last_modified: None,
            temp_is_folder: None,
            created_at: None,
            updated_at: None,
        }
    }

    // Constructor for delete operations
    pub fn new_delete(
        temporary_id: String,
        onedrive_id: String,
    ) -> Self {
        Self {
            id: None,
            temporary_id,
            onedrive_id: Some(onedrive_id),
            change_type: LocalChangeType::Delete,
            status: LocalChangeStatus::New,
            parent_id: None,
            file_name: None,
            old_inode: None,
            new_inode: None,
            old_name: None,
            new_name: None,
            old_etag: None,
            new_etag: None,
            file_size: None,
            mime_type: None,
            temp_created_date: None,
            temp_last_modified: None,
            temp_is_folder: None,
            created_at: None,
            updated_at: None,
        }
    }

    // Validation methods
    pub fn validate(&self) -> Result<()> {
        match self.change_type {
            LocalChangeType::CreateFile | LocalChangeType::CreateFolder => {
                if self.parent_id.is_none() || self.file_name.is_none() {
                    return Err(anyhow!("Create operations require parent_id and file_name"));
                }
            }
            LocalChangeType::Move => {
                if self.old_inode.is_none() || self.new_inode.is_none() {
                    return Err(anyhow!("Move operations require old_inode and new_inode"));
                }
            }
            LocalChangeType::Rename => {
                if self.old_name.is_none() || self.new_name.is_none() {
                    return Err(anyhow!("Rename operations require old_name and new_name"));
                }
            }
            LocalChangeType::Modify => {
                if self.old_etag.is_none() || self.new_etag.is_none() {
                    return Err(anyhow!("Modify operations require old_etag and new_etag"));
                }
            }
            LocalChangeType::Delete => {
                if self.onedrive_id.is_none() {
                    return Err(anyhow!("Delete operations require onedrive_id"));
                }
            }
        }
        Ok(())
    }

    // Helper methods
    pub fn is_create_operation(&self) -> bool {
        matches!(self.change_type, LocalChangeType::CreateFile | LocalChangeType::CreateFolder)
    }

    pub fn is_move_operation(&self) -> bool {
        matches!(self.change_type, LocalChangeType::Move)
    }

    pub fn is_rename_operation(&self) -> bool {
        matches!(self.change_type, LocalChangeType::Rename)
    }

    pub fn is_modify_operation(&self) -> bool {
        matches!(self.change_type, LocalChangeType::Modify)
    }

    pub fn is_delete_operation(&self) -> bool {
        matches!(self.change_type, LocalChangeType::Delete)
    }
}

pub struct LocalChangesRepository {
    pool: Pool<Sqlite>,
}

impl LocalChangesRepository {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    /// Store a local change in the database
    pub async fn store_local_change(&self, change: &LocalChange) -> Result<()> {
        change.validate()?;

        let r = sqlx::query(
            r#"
            INSERT INTO local_changes (
                temporary_id, onedrive_id, change_type, status,
                parent_id, file_name, old_inode, new_inode,
                old_name, new_name, old_etag, new_etag,
                file_size, mime_type, temp_created_date, temp_last_modified, temp_is_folder,
                created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&change.temporary_id)
        .bind(&change.onedrive_id)
        .bind(change.change_type.as_str())
        .bind(change.status.as_str())
        .bind(&change.parent_id)
        .bind(&change.file_name)
        .bind(change.old_inode)
        .bind(change.new_inode)
        .bind(&change.old_name)
        .bind(&change.new_name)
        .bind(&change.old_etag)
        .bind(&change.new_etag)
        .bind(change.file_size)
        .bind(&change.mime_type)
        .bind(&change.temp_created_date)
        .bind(&change.temp_last_modified)
        .bind(change.temp_is_folder)
        .bind(&change.created_at)
        .bind(&change.updated_at)
        .execute(&self.pool)
        .await;
        if r.is_err() {
            error!("Failed to store local change: {}", r.err().unwrap());
            return Err(anyhow!("Failed to store local change"));
        }
        debug!("Stored local change: {} ({})", change.temporary_id, change.change_type.as_str());
        Ok(())
    }

    /// Get local changes by status
    pub async fn get_changes_by_status(&self, status: LocalChangeStatus) -> Result<Vec<LocalChange>> {
        let rows = sqlx::query(
            r#"
            SELECT * FROM local_changes WHERE status = ? ORDER BY created_at ASC
            "#,
        )
        .bind(status.as_str())
        .fetch_all(&self.pool)
        .await?;

        let mut changes = Vec::new();
        for row in rows {
            changes.push(self.row_to_local_change(row)?);
        }
        Ok(changes)
    }

    /// Update change status
    pub async fn update_change_status(&self, id: i64, status: LocalChangeStatus) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE local_changes 
            SET status = ?, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            "#,
        )
        .bind(status.as_str())
        .bind(id)
        .execute(&self.pool)
        .await?;

        debug!("Updated change status: {} -> {}", id, status.as_str());
        Ok(())
    }

    /// Get local changes by parent ID (for move operations)
    pub async fn get_changes_by_parent_id(&self, parent_id: &str) -> Result<Vec<LocalChange>> {
        let rows = sqlx::query(
            r#"
            SELECT * FROM local_changes 
            WHERE parent_id = ?
            ORDER BY created_at ASC
            "#,
        )
        .bind(parent_id)
        .fetch_all(&self.pool)
        .await?;

        let mut changes = Vec::new();
        for row in rows {
            changes.push(self.row_to_local_change(row)?);
        }
        Ok(changes)
    }

    /// Get local changes by OneDrive ID
    pub async fn get_changes_by_onedrive_id(&self, onedrive_id: &str) -> Result<Vec<LocalChange>> {
        let rows = sqlx::query(
            r#"
            SELECT * FROM local_changes WHERE onedrive_id = ? ORDER BY created_at ASC
            "#,
        )
        .bind(onedrive_id)
        .fetch_all(&self.pool)
        .await?;

        let mut changes = Vec::new();
        for row in rows {
            changes.push(self.row_to_local_change(row)?);
        }
        Ok(changes)
    }

    /// Delete a local change
    pub async fn delete_change(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM local_changes WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        debug!("Deleted local change: {}", id);
        Ok(())
    }

    /// Convert database row to LocalChange
    fn row_to_local_change(&self, row: sqlx::sqlite::SqliteRow) -> Result<LocalChange> {
        Ok(LocalChange {
            id: row.try_get("id").ok(),
            temporary_id: row.try_get("temporary_id").unwrap_or_default(),
            onedrive_id: row.try_get("onedrive_id").ok(),
            change_type: LocalChangeType::from_str(
                row.try_get::<String, _>("change_type").unwrap_or_default().as_str()
            ).unwrap_or(LocalChangeType::Modify),
            status: LocalChangeStatus::from_str(
                row.try_get::<String, _>("status").unwrap_or_default().as_str()
            ).unwrap_or(LocalChangeStatus::New),
            parent_id: row.try_get("parent_id").ok(),
            file_name: row.try_get("file_name").ok(),
            old_inode: row.try_get("old_inode").ok(),
            new_inode: row.try_get("new_inode").ok(),
            old_name: row.try_get("old_name").ok(),
            new_name: row.try_get("new_name").ok(),
            old_etag: row.try_get("old_etag").ok(),
            new_etag: row.try_get("new_etag").ok(),
            file_size: row.try_get("file_size").ok(),
            mime_type: row.try_get("mime_type").ok(),
            temp_created_date: row.try_get("temp_created_date").ok(),
            temp_last_modified: row.try_get("temp_last_modified").ok(),
            temp_is_folder: row.try_get("temp_is_folder").ok(),
            created_at: row.try_get("created_at").ok(),
            updated_at: row.try_get("updated_at").ok(),
        })
    }
} 