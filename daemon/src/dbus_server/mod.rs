pub mod server;
pub mod message_handler;

use std::sync::Arc;
use anyhow::Result;
use log::{info, error, debug};
use zbus::connection;
use crate::app_state::AppState;
use server::ServiceImpl;

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
        
        // Create connection and register service using session bus (more appropriate for user apps)
        let connection = connection::Builder::session()?
            .name("org.freedesktop.OneDriveSync")?
            .serve_at("/org/freedesktop/OneDriveSync", service)?
            .build()
            .await?;
        
        self.connection = Some(connection);
        info!("✅ DBus server started successfully with full interface registration on session bus");
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

    /// Check if server is running
    pub fn is_running(&self) -> bool {
        self.connection.is_some()
    }

    /// Get the service implementation for direct method calls
    pub fn get_service(&self) -> ServiceImpl {
        ServiceImpl::new(self.app_state.clone())
    }
}