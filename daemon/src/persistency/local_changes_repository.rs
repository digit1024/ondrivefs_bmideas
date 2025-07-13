//! LocalChangesRepository: Handles local_changes table operations 
use anyhow::{Context, Result};
use log::debug;
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

pub struct LocalChangesRepository {
    pool: Pool<Sqlite>,
}

impl LocalChangesRepository {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    /// Get local changes by parent virtual path
    pub async fn get_local_changes_by_parent_path(&self, parent_virtual_path: &str) -> Result<Vec<LocalChange>> {
        let rows = sqlx::query(
            r#"
            SELECT * FROM local_changes WHERE old_virtual_path = ? OR virtual_path = ?
            "#,
        )
        .bind(parent_virtual_path)
        .bind(parent_virtual_path)
        .fetch_all(&self.pool)
        .await?;

        let mut changes = Vec::new();
        for row in rows {
            let change = LocalChange {
                id: row.try_get("id").ok(),
                temporary_id: row.try_get("temporary_id").unwrap_or_default(),
                onedrive_id: row.try_get("onedrive_id").ok(),
                change_type: LocalChangeType::from_str(row.try_get::<String, _>("change_type").unwrap_or_default().as_str()).unwrap_or(LocalChangeType::Modify),
                virtual_path: row.try_get("virtual_path").unwrap_or_default(),
                old_virtual_path: row.try_get("old_virtual_path").ok(),
                parent_id: row.try_get("parent_id").ok(),
                file_name: row.try_get("file_name").ok(),
                content_file_id: row.try_get("content_file_id").ok(),
                base_etag: row.try_get("base_etag").ok(),
                status: LocalChangeStatus::from_str(row.try_get::<String, _>("status").unwrap_or_default().as_str()).unwrap_or(LocalChangeStatus::New),
                file_hash: row.try_get("file_hash").ok(),
                file_size: row.try_get("file_size").ok(),
                mime_type: row.try_get("mime_type").ok(),
                temp_name: row.try_get("temp_name").ok(),
                temp_size: row.try_get("temp_size").ok(),
                temp_mime_type: row.try_get("temp_mime_type").ok(),
                temp_created_date: row.try_get("temp_created_date").ok(),
                temp_last_modified: row.try_get("temp_last_modified").ok(),
                temp_is_folder: row.try_get("temp_is_folder").ok(),
                error_message: row.try_get("error_message").ok(),
                retry_count: row.try_get("retry_count").unwrap_or(0),
                priority: row.try_get("priority").unwrap_or(0),
                created_at: row.try_get("created_at").ok(),
                updated_at: row.try_get("updated_at").ok(),
            };
            changes.push(change);
        }
        Ok(changes)
    }
} 