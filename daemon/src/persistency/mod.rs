//! Persistency module for OneDrive sync daemon
//!
//! This module provides database functionality for storing OneDrive metadata,
//! sync state, and other persistent data using SQLx with SQLite.

pub mod drive_item_repository;
pub mod drive_item_with_fuse_repository;
pub mod sync_state_repository;
pub mod download_queue_repository;
pub mod upload_queue_repository;
pub mod profile_repository;
pub mod processing_item_repository;
pub mod local_changes_repository;
pub mod fuse_repository;
pub mod types;

use anyhow::{Context, Result};
use log::info;
use sqlx::{Pool, Sqlite};
use std::path::PathBuf;

/// Database manager for OneDrive sync operations
pub struct PersistencyManager {
    pool: Pool<Sqlite>,
    db_path: PathBuf,
}

impl PersistencyManager {
    /// Create a new persistency manager with database connection pool
    pub async fn new(data_dir: PathBuf) -> Result<Self> {
        let db_path = data_dir.join("onedrive.db");

        // Print the path for debugging

        // Ensure data directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Create connection pool
        let database_url = format!("sqlite:{}?mode=rwc", db_path.display());

        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(100)
            .connect(&database_url)
            .await
            .context("Failed to connect to database")?;

        info!(
            "Initialized database connection pool at: {}",
            db_path.display()
        );

        Ok(Self { pool, db_path })
    }

    /// Get the database connection pool
    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    /// Get the database file path
    pub fn db_path(&self) -> &PathBuf {
        &self.db_path
    }

    /// Initialize database schema (create tables if they don't exist)
    pub async fn init_database(&self) -> Result<()> {
        info!("Initializing database schema...");

        // Create tables for OneDrive models
        self.create_drive_items_table().await?;
        self.create_drive_items_with_fuse_table().await?;
        self.create_sync_state_table().await?;
        self.create_download_queue_table().await?;
        self.create_upload_queue_table().await?;
        self.create_user_profiles_table().await?;
        self.create_processing_items_table().await?;
        self.create_local_changes_table().await?;

        info!("Database schema initialized successfully");
        Ok(())
    }

    /// Create the drive_items table for storing OneDrive file/folder metadata
    async fn create_drive_items_table(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS drive_items (
                id TEXT PRIMARY KEY,
                name TEXT,
                etag TEXT,
                last_modified TEXT,
                created_date TEXT,
                size INTEGER,
                is_folder BOOLEAN,
                mime_type TEXT,
                download_url TEXT,
                is_deleted BOOLEAN DEFAULT FALSE,
                parent_id TEXT,
                parent_path TEXT,
                local_path TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Create the drive_items_with_fuse table for storing OneDrive file/folder metadata with Fuse data
    async fn create_drive_items_with_fuse_table(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS drive_items_with_fuse (
                virtual_ino INTEGER PRIMARY KEY AUTOINCREMENT,
                onedrive_id TEXT UNIQUE NOT NULL,
                name TEXT,
                etag TEXT,
                last_modified TEXT,
                created_date TEXT,
                size INTEGER,
                is_folder BOOLEAN,
                mime_type TEXT,
                download_url TEXT,
                is_deleted BOOLEAN DEFAULT FALSE,
                parent_id TEXT,
                parent_path TEXT,
                local_path TEXT,
                parent_ino INTEGER,
                virtual_path TEXT,
                display_path TEXT,
                file_source TEXT,
                sync_status TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create indexes for efficient lookups
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_drive_items_with_fuse_onedrive_id ON drive_items_with_fuse(onedrive_id)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_drive_items_with_fuse_parent_ino ON drive_items_with_fuse(parent_ino)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_drive_items_with_fuse_virtual_path ON drive_items_with_fuse(virtual_path)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_drive_items_with_fuse_file_source ON drive_items_with_fuse(file_source)",
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Create the sync_state table for tracking sync operations
    async fn create_sync_state_table(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS sync_state (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                delta_link TEXT,
                last_sync_time DATETIME,
                sync_status TEXT DEFAULT 'idle',
                error_message TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Create the download_queue table for tracking pending downloads
    async fn create_download_queue_table(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS download_queue (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                drive_item_id TEXT NOT NULL,
                local_path TEXT NOT NULL,
                priority INTEGER DEFAULT 0,
                status TEXT DEFAULT 'pending',
                retry_count INTEGER DEFAULT 0,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (drive_item_id) REFERENCES drive_items (id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Create the upload_queue table for tracking pending uploads
    async fn create_upload_queue_table(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS upload_queue (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                local_path TEXT NOT NULL,
                parent_id TEXT,
                file_name TEXT NOT NULL,
                priority INTEGER DEFAULT 0,
                status TEXT DEFAULT 'pending',
                retry_count INTEGER DEFAULT 0,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Create the user_profiles table for storing user profile information
    async fn create_user_profiles_table(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_profiles (
                id TEXT PRIMARY KEY,
                display_name TEXT,
                given_name TEXT,
                surname TEXT,
                mail TEXT,
                user_principal_name TEXT,
                job_title TEXT,
                business_phones TEXT,
                mobile_phone TEXT,
                office_location TEXT,
                preferred_language TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Create the processing_items table for storing DriveItems with processing status
    async fn create_processing_items_table(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS processing_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                drive_item_id TEXT UNIQUE NOT NULL,
                name TEXT,
                etag TEXT,
                last_modified TEXT,
                created_date TEXT,
                size INTEGER,
                is_folder BOOLEAN,
                mime_type TEXT,
                download_url TEXT,
                is_deleted BOOLEAN,
                parent_id TEXT,
                parent_path TEXT,
                local_path TEXT,
                status TEXT DEFAULT 'new',
                error_message TEXT,
                last_status_update DATETIME DEFAULT CURRENT_TIMESTAMP,
                retry_count INTEGER DEFAULT 0,
                priority INTEGER DEFAULT 0,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create indexes for efficient queries
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_processing_items_status ON processing_items(status)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_processing_items_priority ON processing_items(priority)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_processing_items_status_update ON processing_items(last_status_update)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_processing_items_drive_item_id ON processing_items(drive_item_id)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_processing_items_retry_count ON processing_items(retry_count)")
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Create the local_changes table for storing local file system changes
    async fn create_local_changes_table(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS local_changes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                temporary_id TEXT NOT NULL,     -- "temp_001", "temp_002", etc.
                onedrive_id TEXT,              -- Assigned during API call
                change_type TEXT NOT NULL,     -- 'create_file', 'create_folder', 'modify', 'delete', 'move', 'rename'
                status TEXT DEFAULT 'new',     -- 'new', 'implemented', 'reflected', 'failed'
                
                -- For CREATE operations
                parent_id TEXT,                -- OneDrive parent folder ID
                file_name TEXT,                -- Name for new files/folders
                
                -- For MOVE operations  
                old_inode INTEGER,            -- Original inode
                new_inode INTEGER,            -- New inode
                
                -- For RENAME operations
                old_name TEXT,                 -- Original name
                new_name TEXT,                 -- New name
                
                -- For UPDATE operations
                old_etag TEXT,                 -- Original ETag
                new_etag TEXT,                 -- New ETag
                
                -- File metadata (for all operations)
                file_size INTEGER,
                mime_type TEXT,
                temp_created_date TEXT,
                temp_last_modified TEXT,
                temp_is_folder BOOLEAN,
                
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create indexes for performance
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_local_changes_status ON local_changes(status)")
            .execute(&self.pool)
            .await?;
        
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_local_changes_type ON local_changes(change_type)")
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

impl Drop for PersistencyManager {
    fn drop(&mut self) {
        info!("Closing database connection pool");
    }
}
