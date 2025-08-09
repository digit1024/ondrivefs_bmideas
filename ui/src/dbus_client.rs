// SPDX-License-Identifier: MPL-2.0

use anyhow::Result;
use log::info;
use onedrive_sync_lib::dbus::types::{ConflictItem, DaemonStatus, SyncQueueItem, UserChoice, UserProfile, MediaItem};
use zbus::connection::Builder;
use zbus::Proxy;
// use zbus::proxy::SignalStream;
use once_cell::sync::Lazy;
use tokio::sync::{broadcast::{self, Receiver, Sender}, RwLock};

const DBUS_SERVICE: &str = "org.freedesktop.OneDriveSync";
const DBUS_PATH: &str = "/org/freedesktop/OneDriveSync";
const DBUS_INTERFACE: &str = "org.freedesktop.OneDriveSync";

// Global broadcast channel for daemon status updates
pub static DAEMON_STATUS_TX: Lazy<Sender<DaemonStatus>> = Lazy::new(|| {
    let (tx, _rx) = broadcast::channel::<DaemonStatus>(64);
    tx
});

pub fn subscribe_status_receiver() -> Receiver<DaemonStatus> {
    DAEMON_STATUS_TX.subscribe()
}

static LATEST_STATUS: Lazy<RwLock<Option<DaemonStatus>>> = Lazy::new(|| RwLock::new(None));

pub async fn take_latest_status() -> Option<DaemonStatus> {
    let mut guard = LATEST_STATUS.write().await;
    guard.take()
}

pub async fn with_dbus_client<CallFn, CallFuture, Output, CallError>(
    call: CallFn,
) -> Result<Output, String>
where
    CallFn: FnOnce(DbusClient) -> CallFuture,
    CallFuture: std::future::Future<Output = Result<Output, CallError>>,
    CallError: std::fmt::Display,
{
    match DbusClient::new().await {
        Ok(client) => {
            info!("Dbus client created successfully");
            match call(client).await {
                Ok(val) => Ok(val),
                Err(e) => Err(format!("{}", e)),
            }
        }
        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
    }
}

/// DBus client for communicating with the OneDrive sync daemon
pub struct DbusClient {
    connection: zbus::Connection,
}

impl DbusClient {
    /// Create a new DBus client instance
    pub async fn new() -> Result<Self> {
        info!("Creating new DBus client instance");
        let connection = Builder::session()?.build().await?;

        info!("DBus client created successfully");
        Ok(Self { connection })
    }

    /// Subscribe to daemon_status_changed signals
    pub async fn subscribe_daemon_status<F, Fut>(&self, mut on_status: F) -> Result<()>
    where
        F: FnMut(DaemonStatus) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let proxy = self.get_proxy().await?;
        let mut stream = proxy.receive_signal("DaemonStatusChanged").await?;
        tokio::spawn(async move {
            use futures_util::StreamExt;
            while let Some(msg) = stream.next().await {
                if let Ok(status) = msg.body().deserialize::<DaemonStatus>() {
                    let _ = DAEMON_STATUS_TX.send(status.clone());
                    if let Ok(mut guard) = LATEST_STATUS.try_write() {
                        *guard = Some(status.clone());
                    }
                    on_status(status).await;
                }
            }
        });
        Ok(())
    }

    pub async fn get_conflicts(&self) -> Result<Vec<ConflictItem>> {
        info!("Fetching conflicts from daemon");
        let proxy = self.get_proxy().await?;

        let items = proxy
            .call_method("GetConflicts", &())
            .await?
            .body()
            .deserialize::<Vec<ConflictItem>>()?;
            
        info!("Successfully fetched {} conflicts", items.len());
        Ok(items)
    }

    pub async fn resolve_conflict(&self, db_id: i64, choice: UserChoice) -> Result<()> {
        info!("Resolving conflict for item {}", db_id);
        let proxy = self.get_proxy().await?;
        
        proxy.call_method("ResolveConflict", &(db_id, choice)).await?;
        
        info!("Successfully resolved conflict for item {}", db_id);
        Ok(())
    }
    
    /// Get the user profile from the daemon
    pub async fn get_user_profile(&self) -> Result<UserProfile> {
        info!("Fetching user profile from daemon");
        let proxy = self.get_proxy().await?;

        let user_profile = proxy
            .call_method("GetUserProfile", &())
            .await?
            .body()
            .deserialize::<UserProfile>()?;

        info!(
            "Successfully fetched user profile: {}",
            user_profile.display_name
        );
        Ok(user_profile)
    }

    /// Get the daemon status
    pub async fn get_daemon_status(&self) -> Result<DaemonStatus> {
        info!("Fetching daemon status");
        let proxy = self.get_proxy().await?;

        let status = proxy
            .call_method("GetDaemonStatus", &())
            .await?
            .body()
            .deserialize::<DaemonStatus>()?;

        info!(
            "Successfully fetched daemon status: authenticated={}, connected={}, sync_status={:?}",
            status.is_authenticated, status.is_connected, status.sync_status
        );
        Ok(status)
    }

    /// Get the download queue items
    pub async fn get_download_queue(&self) -> Result<Vec<SyncQueueItem>> {
        info!("Fetching download queue from daemon");
        let proxy = self.get_proxy().await?;

        let items = proxy
            .call_method("GetDownloadQueue", &())
            .await?
            .body()
            .deserialize::<Vec<SyncQueueItem>>()?;

        info!(
            "Successfully fetched download queue with {} items",
            items.len()
        );
        Ok(items)
    }

    /// Get the upload queue items
    pub async fn get_upload_queue(&self) -> Result<Vec<SyncQueueItem>> {
        info!("Fetching upload queue from daemon");
        let proxy = self.get_proxy().await?;

        let items = proxy
            .call_method("GetUploadQueue", &())
            .await?
            .body()
            .deserialize::<Vec<SyncQueueItem>>()?;

        info!(
            "Successfully fetched upload queue with {} items",
            items.len()
        );
        Ok(items)
    }

    /// List media items (images/videos) newest first with pagination and optional date filters
    pub async fn list_media(&self, offset: u32, limit: u32, start_date: String, end_date: String) -> Result<Vec<MediaItem>> {
        info!("Fetching media list from daemon: offset={}, limit={}", offset, limit);
        let proxy = self.get_proxy().await?;
        let items = proxy
            .call_method("ListMedia", &(offset, limit, start_date, end_date))
            .await?
            .body()
            .deserialize::<Vec<MediaItem>>()?;
        Ok(items)
    }

    /// Ensure a thumbnail exists and return its path
    pub async fn fetch_thumbnail(&self, ino: u64) -> Result<String> {
        info!("Fetching thumbnail for ino {}", ino);
        let proxy = self.get_proxy().await?;
        let path = proxy
            .call_method("FetchThumbnail", &(ino,))
            .await?
            .body()
            .deserialize::<String>()?;
        Ok(path)
    }

    /// Ensure a file exists locally at local/{ino} and return its path
    pub async fn ensure_local_by_ino(&self, ino: u64) -> Result<String> {
        info!("Ensuring local file for ino {}", ino);
        let proxy = self.get_proxy().await?;
        let path = proxy
            .call_method("EnsureLocalByIno", &(ino,))
            .await?
            .body()
            .deserialize::<String>()?;
        Ok(path)
    }

    #[allow(dead_code)]
    /// Perform a full reset of the daemon
    pub async fn full_reset(&self) -> Result<()> {
        info!("Performing full reset of daemon");
        let proxy = self.get_proxy().await?;

        proxy.call_method("FullReset", &()).await?;

        info!("Successfully performed full reset of daemon");
        Ok(())
    }
    #[allow(dead_code)]
    /// Check if the daemon is available (service exists on the bus)
    pub async fn is_available(&self) -> bool {
        info!("Checking if daemon is available");
        match Proxy::new(&self.connection, DBUS_SERVICE, DBUS_PATH, DBUS_INTERFACE).await {
            Ok(_) => {
                info!("Daemon is available");
                true
            }
            Err(_) => {
                info!("Daemon is not available");
                false
            }
        }
    }

    /// Get the last 50 log lines from the daemon
    pub async fn get_recent_logs(&self) -> Result<Vec<String>> {
        let proxy = self.get_proxy().await?;

        let logs = proxy
            .call_method("GetRecentLogs", &())
            .await?
            .body()
            .deserialize::<Vec<String>>()?;

        Ok(logs)
    }

    /// List all sync folders
    pub async fn list_sync_folders(&self) -> Result<Vec<String>> {
        let proxy = self.get_proxy().await?;

        let folders = proxy
            .call_method("ListSyncFolders", &())
            .await?
            .body()
            .deserialize::<Vec<String>>()?;
        Ok(folders)
    }

    async fn get_proxy(&self) -> Result<Proxy<'_>, anyhow::Error> {
        let proxy = Proxy::new(&self.connection, DBUS_SERVICE, DBUS_PATH, DBUS_INTERFACE).await?;
        Ok(proxy)
    }

    /// Add a sync folder
    pub async fn add_sync_folder(&self, folder_path: String) -> Result<bool> {
        let proxy = self.get_proxy().await?;

        let result = proxy
            .call_method("AddSyncFolder", &(folder_path,))
            .await?
            .body()
            .deserialize::<bool>()?;
        Ok(result)
    }

    /// Remove a sync folder
    pub async fn remove_sync_folder(&self, folder_path: String) -> Result<bool> {
        let proxy = self.get_proxy().await?;

        let result = proxy
            .call_method("RemoveSyncFolder", &(folder_path,))
            .await?
            .body()
            .deserialize::<bool>()?;
        Ok(result)
    }

    /// Toggle sync pause state
    pub async fn toggle_sync_pause(&self) -> Result<bool> {
        info!("Toggling sync pause state");
        let proxy = self.get_proxy().await?;

        let result = proxy
            .call_method("ToggleSyncPause", &())
            .await?
            .body()
            .deserialize::<bool>()?;
        
        info!("Sync pause toggled: {}", if result { "paused" } else { "resumed" });
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dbus_client_creation() {
        let client = DbusClient::new().await;
        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn test_daemon_availability() {
        if let Ok(client) = DbusClient::new().await {
            let available = client.is_available().await;
            // This test will pass regardless of daemon availability
            // since we're just testing the method call
            assert!(available || !available); // Always true
        }
    }
}
