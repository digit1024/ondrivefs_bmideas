//! Persistency module for OneDrive sync daemon
//!
//! This module provides database functionality for storing OneDrive metadata,
//! sync state, and other persistent data using SQLx with SQLite.

pub mod cached_drive_item_with_fuse_repository;
pub mod download_queue_repository;
pub mod drive_item_with_fuse_repository;
pub mod sync_state_repository;

pub mod processing_item_repository;
pub mod profile_repository;

pub mod types;

use anyhow::{Context, Result};
use log::info;
use sqlx::{Pool, Sqlite};
use std::path::PathBuf;
use std::sync::OnceLock;

/// Database manager for OneDrive sync operations
pub struct PersistencyManager {
    pool: Pool<Sqlite>,
    db_path: PathBuf,
    // Singleton repository instances
    processing_item_repo: OnceLock<processing_item_repository::ProcessingItemRepository>,
    sync_state_repo: OnceLock<sync_state_repository::SyncStateRepository>,
    drive_item_with_fuse_repo: OnceLock<drive_item_with_fuse_repository::DriveItemWithFuseRepository>,
    download_queue_repo: OnceLock<download_queue_repository::DownloadQueueRepository>,
    user_profile_repo: OnceLock<profile_repository::ProfileRepository>,
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

        // Create connection pool with WAL mode for concurrent reads/writes
        // WAL mode allows readers while writing, crucial for FUSE + Sync concurrency
        let database_url = format!("sqlite:{}?mode=rwc", db_path.display());

        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(5) // Allow multiple connections for FUSE + Sync
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    // Enable WAL mode for better concurrency
                    sqlx::query("PRAGMA journal_mode = WAL")
                        .execute(&mut *conn)
                        .await?;
                    // Set busy timeout to wait instead of failing immediately
                    sqlx::query("PRAGMA busy_timeout = 5000")
                        .execute(&mut *conn)
                        .await?;
                    // Optimize for performance
                    sqlx::query("PRAGMA synchronous = NORMAL")
                        .execute(&mut *conn)
                        .await?;
                    // Disable FK checks for better write performance (we handle order manually)
                    sqlx::query("PRAGMA foreign_keys = OFF")
                        .execute(&mut *conn)
                        .await?;
                    Ok(())
                })
            })
            .connect(&database_url)
            .await
            .context("Failed to connect to database")?;

        info!(
            "Initialized database connection pool (WAL mode) at: {}",
            db_path.display()
        );

        Ok(Self { 
            pool, 
            db_path,
            processing_item_repo: OnceLock::new(),
            sync_state_repo: OnceLock::new(),
            drive_item_with_fuse_repo: OnceLock::new(),
            download_queue_repo: OnceLock::new(),
            user_profile_repo: OnceLock::new(),
        })
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

        self.create_drive_items_with_fuse_table().await?;
        self.create_sync_state_table().await?;
        self.create_download_queue_table().await?;
        self.create_user_profiles_table().await?;
        self.create_processing_items_table().await?;

        info!("Database schema initialized successfully");
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
                ctag TEXT,
                last_modified TEXT,
                created_date TEXT,
                size INTEGER,
                is_folder BOOLEAN,
                mime_type TEXT,
                download_url TEXT,
                is_deleted BOOLEAN DEFAULT FALSE,
                parent_id TEXT,
                parent_path TEXT,
                parent_ino INTEGER,
                virtual_path TEXT,
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

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_drive_items_with_fuse_ctag ON drive_items_with_fuse(ctag)",
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
                FOREIGN KEY (drive_item_id) REFERENCES drive_items_with_fuse(onedrive_id)
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

    /// Create the processing_items table for storing items to be processed
    async fn create_processing_items_table(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS processing_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                drive_item_id TEXT NOT NULL,
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
                status TEXT DEFAULT 'new',
                local_path TEXT,
                error_message TEXT,
                last_status_update TEXT,
                retry_count INTEGER DEFAULT 0,
                priority INTEGER DEFAULT 0,
                change_type TEXT DEFAULT 'remote',
                change_operation TEXT DEFAULT 'create',
                conflict_resolution TEXT,
                validation_errors TEXT,
                user_decision TEXT,
                created_at TEXT DEFAULT (datetime('now')),
                updated_at TEXT DEFAULT (datetime('now'))
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_processing_items_status ON processing_items(status)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_processing_items_change_type ON processing_items(change_type)")
        .execute(&self.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_processing_items_priority ON processing_items(priority)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_processing_items_retry_count ON processing_items(retry_count)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_processing_items_drive_item_id ON processing_items(drive_item_id)")
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Get the processing item repository (singleton)
    pub fn processing_item_repository(
        &self,
    ) -> processing_item_repository::ProcessingItemRepository {
        self.processing_item_repo.get_or_init(|| {
            processing_item_repository::ProcessingItemRepository::new(self.pool.clone())
        }).clone()
    }

    /// Get the sync state repository (singleton)  
    pub fn sync_state_repository(&self) -> sync_state_repository::SyncStateRepository {
        self.sync_state_repo.get_or_init(|| {
            sync_state_repository::SyncStateRepository::new(self.pool.clone())
        }).clone()
    }

    /// Get the drive item with fuse repository (singleton)
    pub fn drive_item_with_fuse_repository(
        &self,
    ) -> drive_item_with_fuse_repository::DriveItemWithFuseRepository {
        self.drive_item_with_fuse_repo.get_or_init(|| {
            drive_item_with_fuse_repository::DriveItemWithFuseRepository::new(self.pool.clone())
        }).clone()
    }

    /// Get the download queue repository (singleton)
    pub fn download_queue_repository(&self) -> download_queue_repository::DownloadQueueRepository {
        self.download_queue_repo.get_or_init(|| {
            download_queue_repository::DownloadQueueRepository::new(self.pool.clone())
        }).clone()
    }

    /// Get the user profile repository (singleton)
    pub fn user_profile_repository(&self) -> profile_repository::ProfileRepository {
        self.user_profile_repo.get_or_init(|| {
            profile_repository::ProfileRepository::new(self.pool.clone())
        }).clone()
    }
}

impl Drop for PersistencyManager {
    fn drop(&mut self) {
        info!("Closing database connection pool");
    }
}
