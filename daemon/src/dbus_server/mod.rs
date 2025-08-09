pub mod message_handler;
pub mod server;

use crate::app_state::AppState;
use anyhow::Result;
use log::{debug, error, info};
use server::ServiceImpl;
use std::sync::Arc;
use zbus::connection;
use zbus::object_server::SignalEmitter;
use onedrive_sync_lib::dbus::types::DaemonStatus;

async fn compute_status(app_state: &Arc<AppState>) -> DaemonStatus {
    use onedrive_sync_lib::dbus::types::SyncStatus;
    let is_authenticated = app_state.auth().get_valid_token().await.is_ok();
    let is_connected = matches!(
        app_state.connectivity().check_connectivity().await,
        crate::connectivity::ConnectivityStatus::Online
    );
    let sync_status = if let Some(metrics) = app_state.scheduler().get_task_metrics("sync_cycle").await {
        if metrics.is_running { SyncStatus::Running } else { SyncStatus::Paused }
    } else { SyncStatus::Paused };
    let has_conflicts = app_state
        .persistency()
        .processing_item_repository()
        .get_processing_items_by_status(
            &crate::persistency::processing_item_repository::ProcessingStatus::Conflicted,
        )
        .await
        .map(|items| !items.is_empty())
        .unwrap_or(false);
    let path_str = format!("{}/OneDrive", std::env::var("HOME").unwrap_or_default());
    let p = std::path::Path::new(&path_str);
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    let is_mounted = mounts
        .lines()
        .any(|line| line.split_whitespace().nth(1) == Some(p.to_str().unwrap_or_default()));
    DaemonStatus { is_authenticated, is_connected, sync_status, has_conflicts, is_mounted }
}

pub struct DbusServerManager {
    app_state: Arc<AppState>,
    connection: Option<zbus::Connection>,
}

impl DbusServerManager {
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self {
            app_state,
            connection: None,
        }
    }

    /// Start the DBus server
    pub async fn start(&mut self) -> Result<()> {
        info!("🚀 Starting DBus server...");

        // Create service implementation
        let service = ServiceImpl::new(self.app_state.clone());

        // Create connection
        let connection = connection::Builder::session()?
            .name("org.freedesktop.OneDriveSync")?
            .serve_at("/org/freedesktop/OneDriveSync", service)?
            .build()
            .await?;

        // Spawn periodic status signal emitter (change-detected every 10s)
        let app_state_clone = self.app_state.clone();
        let connection_clone = connection.clone();
        tokio::spawn(async move {
            let task = crate::tasks::status_broadcast::StatusBroadcastTask::new(app_state_clone, connection_clone);
            task.run().await;
        });

        self.connection = Some(connection);
        info!(
            "✅ DBus server started successfully with full interface registration on session bus"
        );
        Ok(())
    }

    /// Stop the DBus server
    pub async fn stop(&mut self) -> Result<()> {
        if let Some(connection) = &self.connection {
            info!("🛑 Stopping DBus server...");

            // The connection will be dropped automatically, releasing the bus name
            self.connection = None;
            info!("✅ DBus server stopped successfully");
        }
        Ok(())
    }
    #[allow(dead_code)]
    /// Check if server is running
    pub fn is_running(&self) -> bool {
        self.connection.is_some()
    }
    #[allow(dead_code)]
    /// Get the service implementation for direct method calls
    pub fn get_service(&self) -> ServiceImpl {
        ServiceImpl::new(self.app_state.clone())
    }
}
