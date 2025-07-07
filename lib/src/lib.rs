//! OneDrive sync DBus library

pub mod client;
pub mod dbus_interface;
pub mod types;

// Re-export main types
pub use client::OneDriveSyncClient;
pub use types::{SyncError, SyncMetrics, SyncProgress, SyncStatus};

/// Create a new DBus client
pub async fn create_client() -> anyhow::Result<OneDriveSyncClient> {
    OneDriveSyncClient::new().await
} 