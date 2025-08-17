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
