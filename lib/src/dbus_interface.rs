//! DBus interface definition for OneDrive sync

use crate::types::{SyncError, SyncProgress, SyncStatus};

/// DBus bus name and object path constants
pub const DBUS_BUS_NAME: &str = "org.freedesktop.OneDriveSync";
pub const DBUS_OBJECT_PATH: &str = "/org/freedesktop/OneDriveSync";

/// DBus interface for OneDrive sync operations
pub trait OneDriveSync {
    // ===== Sync Folder Management =====
    
    /// Get all sync folders
    async fn get_all_sync_folders(&self) -> Result<Vec<String>, SyncError>;
    
    /// Add a sync folder
    async fn add_sync_folder(&self, folder: String) -> Result<(), SyncError>;
    
    /// Remove a sync folder
    async fn remove_sync_folder(&self, folder: String) -> Result<(), SyncError>;
    
    // ===== Sync Control =====
    
    /// Pause syncing
    async fn pause_syncing(&self) -> Result<(), SyncError>;
    
    /// Resume syncing
    async fn resume_syncing(&self) -> Result<(), SyncError>;
    
    // ===== Status & Info =====
    
    /// Get sync status
    async fn get_sync_status(&self) -> Result<String, SyncError>;
    
    /// Get sync progress
    async fn get_sync_progress(&self) -> Result<(u32, u32), SyncError>;
    
    /// Get download queue size
    async fn get_download_queue_size(&self) -> Result<u32, SyncError>;
    
    /// Get last sync time
    async fn get_last_sync_time(&self) -> Result<String, SyncError>;
    
    // ===== Configuration =====
    
    /// Get mount point
    async fn get_mount_point(&self) -> Result<String, SyncError>;
    
    /// Set mount point
    async fn set_mount_point(&self, path: String) -> Result<(), SyncError>;
    
    // ===== File Operations =====
    
    /// Upload a file
    async fn upload_file(&self, file_path: String) -> Result<(), SyncError>;
}

 