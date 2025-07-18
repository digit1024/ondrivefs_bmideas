//! DBus client implementation for OneDrive sync

use crate::dbus_interface::{DBUS_BUS_NAME, DBUS_OBJECT_PATH};
use crate::types::{ SyncProgress, SyncStatus};
use anyhow::Result;
use zbus::{Connection, Proxy};

/// DBus client for OneDrive sync operations
pub struct DbusSyncClient {
    connection: Connection,
    proxy: Proxy<'static>,
}

impl DbusSyncClient {
    /// Create a new DBus client
    pub async fn new() -> Result<Self> {
        let connection = Connection::session().await?;
        let proxy = Proxy::new(
            &connection,
            DBUS_BUS_NAME,
            DBUS_OBJECT_PATH,
            "org.freedesktop.OneDriveSync",
        ).await?;
        
        Ok(Self {
            connection,
            proxy,
        })
    }
    
    /// Get all sync folders
    pub async fn get_all_sync_folders(&self) -> Result<Vec<String>> {
        let result: Vec<String> = self.proxy.call("GetAllSyncFolders", &()).await?;
        Ok(result)
    }
    
    /// Add a sync folder
    pub async fn add_sync_folder(&self, folder: String) -> Result<()> {
        let _: () = self.proxy.call("AddSyncFolder", &(folder,)).await?;
        Ok(())
    }
    
    /// Remove a sync folder
    pub async fn remove_sync_folder(&self, folder: String) -> Result<()> {
        let _: () = self.proxy.call("RemoveSyncFolder", &(folder,)).await?;
        Ok(())
    }
    
    /// Pause syncing
    pub async fn pause_syncing(&self) -> Result<()> {
        let _: () = self.proxy.call("PauseSyncing", &()).await?;
        Ok(())
    }
    
    /// Resume syncing
    pub async fn resume_syncing(&self) -> Result<()> {
        let _: () = self.proxy.call("ResumeSyncing", &()).await?;
        Ok(())
    }
    
    /// Get sync status
    pub async fn get_sync_status(&self) -> Result<String> {
        let result: String = self.proxy.call("GetSyncStatus", &()).await?;
        Ok(result)
    }
    
    /// Get sync progress
    pub async fn get_sync_progress(&self) -> Result<(u32, u32)> {
        let result: (u32, u32) = self.proxy.call("GetSyncProgress", &()).await?;
        Ok(result)
    }
    
    /// Get download queue size
    pub async fn get_download_queue_size(&self) -> Result<u32> {
        let result: u32 = self.proxy.call("GetDownloadQueueSize", &()).await?;
        Ok(result)
    }
    
    /// Get last sync time
    pub async fn get_last_sync_time(&self) -> Result<String> {
        let result: String = self.proxy.call("GetLastSyncTime", &()).await?;
        Ok(result)
    }
    
    /// Get mount point
    pub async fn get_mount_point(&self) -> Result<String> {
        let result: String = self.proxy.call("GetMountPoint", &()).await?;
        Ok(result)
    }
    
    /// Set mount point
    pub async fn set_mount_point(&self, path: String) -> Result<()> {
        let _: () = self.proxy.call("SetMountPoint", &(path,)).await?;
        Ok(())
    }
    
    /// Upload a file
    pub async fn upload_file(&self, file_path: String) -> Result<()> {
        let _: () = self.proxy.call("UploadFile", &(file_path,)).await?;
        Ok(())
    }
}

impl Clone for DbusSyncClient {
    fn clone(&self) -> Self {
        Self {
            connection: self.connection.clone(),
            proxy: self.proxy.clone(),
        }
    }
}

/// Convenience methods for common operations
impl DbusSyncClient {
    /// Get sync metrics in a structured format
    pub async fn get_sync_metrics(&self) -> Result<crate::types::SyncMetrics> {
        let status_str = self.get_sync_status().await?;
        let status = match status_str.as_str() {
            "running" => SyncStatus::Running,
            "paused" => SyncStatus::Paused,
            _ => SyncStatus::Error(status_str),
        };
        
        let (current_files, total_files) = self.get_sync_progress().await?;
        let queue_size = self.get_download_queue_size().await?;
        let last_sync_time = self.get_last_sync_time().await.ok();
        let sync_folders = self.get_all_sync_folders().await?;
        
        Ok(crate::types::SyncMetrics {
            status,
            progress: SyncProgress {
                current_files,
                total_files,
                current_bytes: 0, // TODO: Implement byte tracking
                total_bytes: 0,
            },
            queue_size,
            last_sync_time,
            sync_folders,
        })
    }
    
    /// Check if sync is running
    pub async fn is_syncing(&self) -> Result<bool> {
        let status = self.get_sync_status().await?;
        Ok(status == "running")
    }
    
    /// Toggle sync state
    pub async fn toggle_sync(&self) -> Result<()> {
        if self.is_syncing().await? {
            self.pause_syncing().await
        } else {
            self.resume_syncing().await
        }
    }
} 