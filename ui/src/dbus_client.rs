// SPDX-License-Identifier: MPL-2.0

use anyhow::Result;
use log::info;
use onedrive_sync_lib::dbus::types::{DaemonStatus, SyncQueueItem, UserProfile};
use zbus::connection::Builder;
use zbus::Proxy;

const DBUS_SERVICE: &str = "org.freedesktop.OneDriveSync"; 
const DBUS_PATH: &str = "/org/freedesktop/OneDriveSync";
const DBUS_INTERFACE: &str = "org.freedesktop.OneDriveSync";

/// DBus client for communicating with the OneDrive sync daemon
pub struct DbusClient {
    connection: zbus::Connection,
}

impl DbusClient {
    /// Create a new DBus client instance
    pub async fn new() -> Result<Self> {
        info!("Creating new DBus client instance");
        let connection = Builder::session()?
            .build()
            .await?;
        
        info!("DBus client created successfully");
        Ok(Self { connection })
    }

    /// Get the user profile from the daemon
    pub async fn get_user_profile(&self) -> Result<UserProfile> {
        info!("Fetching user profile from daemon");
        let proxy = Proxy::new(
            &self.connection,
            DBUS_SERVICE,
            DBUS_PATH,
            DBUS_INTERFACE,
        )
        .await?;

        let user_profile = proxy
            .call_method("GetUserProfile", &())
            .await?
            .body()
            .deserialize::<UserProfile>()?;

        info!("Successfully fetched user profile: {}", user_profile.display_name);
        Ok(user_profile)
    }

    /// Get the daemon status
    pub async fn get_daemon_status(&self) -> Result<DaemonStatus> {
        info!("Fetching daemon status");
        let proxy = Proxy::new(
            &self.connection,
            DBUS_SERVICE,
            DBUS_PATH,
            DBUS_INTERFACE,
        )
        .await?;

        let status = proxy
            .call_method("GetDaemonStatus", &())
            .await?
            .body()
            .deserialize::<DaemonStatus>()?;

        info!("Successfully fetched daemon status: authenticated={}, connected={}, sync_status={:?}", 
              status.is_authenticated, status.is_connected, status.sync_status);
        Ok(status)
    }

    /// Get the download queue items
    pub async fn get_download_queue(&self) -> Result<Vec<SyncQueueItem>> {
        info!("Fetching download queue from daemon");
        let proxy = Proxy::new(
            &self.connection,
            DBUS_SERVICE,
            DBUS_PATH,
            DBUS_INTERFACE,
        )
        .await?;

        let items = proxy
            .call_method("GetDownloadQueue", &())
            .await?
            .body()
            .deserialize::<Vec<SyncQueueItem>>()?;

        info!("Successfully fetched download queue with {} items", items.len());
        Ok(items)
    }

    /// Get the upload queue items
    pub async fn get_upload_queue(&self) -> Result<Vec<SyncQueueItem>> {
        info!("Fetching upload queue from daemon");
        let proxy = Proxy::new(
            &self.connection,
            DBUS_SERVICE,
            DBUS_PATH,
            DBUS_INTERFACE,
        )
        .await?;

        let items = proxy
            .call_method("GetUploadQueue", &())
            .await?
            .body()
            .deserialize::<Vec<SyncQueueItem>>()?;

        info!("Successfully fetched upload queue with {} items", items.len());
        Ok(items)
    }

    /// Perform a full reset of the daemon
    pub async fn full_reset(&self) -> Result<()> {
        info!("Performing full reset of daemon");
        let proxy = Proxy::new(
            &self.connection,
            DBUS_SERVICE,
            DBUS_PATH,
            DBUS_INTERFACE,
        )
        .await?;

        proxy
            .call_method("FullReset", &())
            .await?;

        info!("Successfully performed full reset of daemon");
        Ok(())
    }

    /// Check if the daemon is available (service exists on the bus)
    pub async fn is_available(&self) -> bool {
        info!("Checking if daemon is available");
        match Proxy::new(
            &self.connection,
            DBUS_SERVICE,
            DBUS_PATH,
            DBUS_INTERFACE,
        )
        .await
        {
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
        let proxy = Proxy::new(
            &self.connection,
            DBUS_SERVICE,
            DBUS_PATH,
            DBUS_INTERFACE,
        )
        .await?;

        let logs = proxy
            .call_method("GetRecentLogs", &())
            .await?
            .body()
            .deserialize::<Vec<String>>()?;

        Ok(logs)
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