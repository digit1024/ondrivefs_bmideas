//! Persistency module for OneDrive sync daemon
//! 
//! This module provides database functionality for storing OneDrive metadata,
//! sync state, and other persistent data using SQLx with SQLite.

pub mod database;

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
            .await.context("Failed to connect to database")?;
            
        info!("Initialized database connection pool at: {}", db_path.display());
        
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
        self.create_sync_state_table().await?;
        self.create_download_queue_table().await?;
        self.create_upload_queue_table().await?;
        
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
}

impl Drop for PersistencyManager {
    fn drop(&mut self) {
        info!("Closing database connection pool");
    }
} 