// SPDX-License-Identifier: MPL-2.0

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use zbus::Connection;

/// DBus bus name and object path constants
pub const DBUS_BUS_NAME: &str = "org.freedesktop.OneDriveSync";
pub const DBUS_OBJECT_PATH: &str = "/org/freedesktop/OneDriveSync";

/// Sync status enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncStatus {
    Running,
    Paused,
    Error(String),
}

impl std::fmt::Display for SyncStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncStatus::Running => write!(f, "running"),
            SyncStatus::Paused => write!(f, "paused"),
            SyncStatus::Error(e) => write!(f, "error: {}", e),
        }
    }
}

/// Sync progress information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncProgress {
    pub current_files: u32,
    pub total_files: u32,
    pub current_bytes: u64,
    pub total_bytes: u64,
}

/// Error types for DBus operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncError {
    AuthenticationFailed(String),
    NetworkError(String),
    FileSystemError(String),
    ConfigurationError(String),
    UnknownError(String),
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::AuthenticationFailed(msg) => write!(f, "Authentication failed: {}", msg),
            SyncError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            SyncError::FileSystemError(msg) => write!(f, "File system error: {}", msg),
            SyncError::ConfigurationError(msg) => write!(f, "Configuration error: {}", msg),
            SyncError::UnknownError(msg) => write!(f, "Unknown error: {}", msg),
        }
    }
}

/// DBus client for OneDrive sync operations
pub struct OneDriveSyncClient {
    connection: Connection,
}

impl OneDriveSyncClient {
    /// Create a new DBus client
    pub async fn new() -> Result<Self> {
        let connection = Connection::session().await?;
        Ok(Self { connection })
    }

    /// Get all sync folders
    pub async fn get_all_sync_folders(&self) -> Result<Vec<String>> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            DBUS_BUS_NAME,
            DBUS_OBJECT_PATH,
            "org.freedesktop.OneDriveSync",
        )
        .await?;

        let folders: Vec<String> = proxy.call("GetAllSyncFolders", &()).await?;
        Ok(folders)
    }

    /// Add a sync folder
    pub async fn add_sync_folder(&self, folder: String) -> Result<()> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            DBUS_BUS_NAME,
            DBUS_OBJECT_PATH,
            "org.freedesktop.OneDriveSync",
        )
        .await?;

        proxy.call("AddSyncFolder", &(folder,)).await?;
        Ok(())
    }

    /// Remove a sync folder
    pub async fn remove_sync_folder(&self, folder: String) -> Result<()> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            DBUS_BUS_NAME,
            DBUS_OBJECT_PATH,
            "org.freedesktop.OneDriveSync",
        )
        .await?;

        proxy.call("RemoveSyncFolder", &(folder,)).await?;
        Ok(())
    }

    /// Pause syncing
    pub async fn pause_syncing(&self) -> Result<()> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            DBUS_BUS_NAME,
            DBUS_OBJECT_PATH,
            "org.freedesktop.OneDriveSync",
        )
        .await?;

        proxy.call("PauseSyncing", &()).await?;
        Ok(())
    }

    /// Resume syncing
    pub async fn resume_syncing(&self) -> Result<()> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            DBUS_BUS_NAME,
            DBUS_OBJECT_PATH,
            "org.freedesktop.OneDriveSync",
        )
        .await?;

        proxy.call("ResumeSyncing", &()).await?;
        Ok(())
    }

    /// Get sync status
    pub async fn get_sync_status(&self) -> Result<String> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            DBUS_BUS_NAME,
            DBUS_OBJECT_PATH,
            "org.freedesktop.OneDriveSync",
        )
        .await?;

        let status: String = proxy.call("GetSyncStatus", &()).await?;
        Ok(status)
    }

    /// Get sync progress
    pub async fn get_sync_progress(&self) -> Result<(u32, u32)> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            DBUS_BUS_NAME,
            DBUS_OBJECT_PATH,
            "org.freedesktop.OneDriveSync",
        )
        .await?;

        let progress: (u32, u32) = proxy.call("GetSyncProgress", &()).await?;
        Ok(progress)
    }

    /// Get download queue size
    pub async fn get_download_queue_size(&self) -> Result<u32> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            DBUS_BUS_NAME,
            DBUS_OBJECT_PATH,
            "org.freedesktop.OneDriveSync",
        )
        .await?;

        let queue_size: u32 = proxy.call("GetDownloadQueueSize", &()).await?;
        Ok(queue_size)
    }

    /// Get last sync time
    pub async fn get_last_sync_time(&self) -> Result<String> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            DBUS_BUS_NAME,
            DBUS_OBJECT_PATH,
            "org.freedesktop.OneDriveSync",
        )
        .await?;

        let last_sync: String = proxy.call("GetLastSyncTime", &()).await?;
        Ok(last_sync)
    }

    /// Get mount point
    pub async fn get_mount_point(&self) -> Result<String> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            DBUS_BUS_NAME,
            DBUS_OBJECT_PATH,
            "org.freedesktop.OneDriveSync",
        )
        .await?;

        let mount_point: String = proxy.call("GetMountPoint", &()).await?;
        Ok(mount_point)
    }

    /// Set mount point
    pub async fn set_mount_point(&self, path: String) -> Result<()> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            DBUS_BUS_NAME,
            DBUS_OBJECT_PATH,
            "org.freedesktop.OneDriveSync",
        )
        .await?;

        proxy.call("SetMountPoint", &(path,)).await?;
        Ok(())
    }

    /// Upload a file
    pub async fn upload_file(&self, file_path: String) -> Result<()> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            DBUS_BUS_NAME,
            DBUS_OBJECT_PATH,
            "org.freedesktop.OneDriveSync",
        )
        .await?;

        proxy.call("UploadFile", &(file_path,)).await?;
        Ok(())
    }
} 