//! DBus server implementation for OneDrive sync daemon

use crate::config::{Settings, SyncConfig};
use crate::file_manager::DefaultFileManager;
use crate::onedrive_service::onedrive_client::OneDriveClient;
use crate::sync::sync_service::SyncService;

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use zbus::{connection, interface};

/// DBus bus name and object path constants
pub const DBUS_BUS_NAME: &str = "org.freedesktop.OneDriveSync";
pub const DBUS_OBJECT_PATH: &str = "/org/freedesktop/OneDriveSync";

/// Server implementation for OneDrive sync DBus interface
#[derive(Clone)]
pub struct OneDriveSyncDaemonServer {
    
    settings: Arc<RwLock<Settings>>,
}

impl OneDriveSyncDaemonServer {
    /// Create a new server instance
    pub fn new(settings: Settings) -> Self {
        Self {
            
            settings: Arc::new(RwLock::new(settings)),
        }
    }
    
    /// Set the sync service
    
    
    
    
    
    
    /// Start the DBus server
    pub async fn start(&self) -> Result<()> {
        let server = self.clone();
        
        let _conn = connection::Builder::session()?
            .name(DBUS_BUS_NAME)?
            .serve_at(DBUS_OBJECT_PATH, server)?
            .build()
            .await?;

        // Keep the connection alive
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }
}

// Implement the DBus interface methods
#[interface(name = "org.freedesktop.OneDriveSync")]
impl OneDriveSyncDaemonServer {
    /// Get all sync folders
    async fn get_all_sync_folders(&self) -> Result<Vec<String>, zbus::fdo::Error> {
        let settings = self.settings.read().await;
        Ok(settings.sync_folders.clone())
    }
    
    /// Add a sync folder
    async fn add_sync_folder(&self, folder: String) -> Result<(), zbus::fdo::Error> {
        let mut settings = self.settings.write().await;
        if !settings.sync_folders.contains(&folder) {
            settings.sync_folders.push(folder);
        }
        Ok(())
    }
    
    /// Remove a sync folder
    async fn remove_sync_folder(&self, folder: String) -> Result<(), zbus::fdo::Error> {
        let mut settings = self.settings.write().await;
        settings.sync_folders.retain(|f| f != &folder);
        Ok(())
    }
    
    /// Pause syncing
    async fn pause_syncing(&self) -> Result<(), zbus::fdo::Error> {
            // TODO: Implement pause method on SyncService
            log::info!("Pausing sync service");
        
        Ok(())
    }
    
    /// Resume syncing
    async fn resume_syncing(&self) -> Result<(), zbus::fdo::Error> {
        
            // TODO: Implement resume method on SyncService
            log::info!("Resuming sync service");
        
        Ok(())
    }
    
    /// Get sync status
    async fn get_sync_status(&self) -> Result<String, zbus::fdo::Error> {
        
            // TODO: Implement is_running method on SyncService
            Ok("running".to_string())
        
            
        
    }
    
    /// Get sync progress
    async fn get_sync_progress(&self) -> Result<(u32, u32), zbus::fdo::Error> {
        // TODO: Implement actual progress tracking
        Ok((0, 0))
    }
    
    /// Get download queue size
    async fn get_download_queue_size(&self) -> Result<u32, zbus::fdo::Error> {
        // TODO: Implement actual queue size tracking
        Ok(0)
    }
    
    /// Get last sync time
    async fn get_last_sync_time(&self) -> Result<String, zbus::fdo::Error> {
        // TODO: Implement actual last sync time tracking
        Ok("Never".to_string())
    }
    
    /// Get mount point
    async fn get_mount_point(&self) -> Result<String, zbus::fdo::Error> {
        // TODO: Add mount_point field to Settings
        Ok("/tmp/onedrive".to_string())
    }
    
    /// Set mount point
    async fn set_mount_point(&self, path: String) -> Result<(), zbus::fdo::Error> {
        // TODO: Add mount_point field to Settings
        log::info!("Setting mount point to: {}", path);
        Ok(())
    }
    
    /// Upload a file
    async fn upload_file(&self, file_path: String) -> Result<(), zbus::fdo::Error> {
        
            // TODO: Implement actual file upload
            log::info!("Uploading file: {}", file_path);
        
        Ok(())
    }
} 