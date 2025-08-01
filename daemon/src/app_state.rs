use std::sync::Arc;

use anyhow::{Context, Result};
use onedrive_sync_lib::config::ProjectConfig;

use crate::{
    auth::onedrive_auth::OneDriveAuth, connectivity::ConnectivityChecker,
    onedrive_service::onedrive_client::OneDriveClient, persistency::PersistencyManager,
    file_manager::DefaultFileManager, message_broker::MessageBroker,
    scheduler::periodic_scheduler::PeriodicScheduler,
};

/// Application state containing all shared components
#[derive(Clone)]
pub struct AppState {
    /// Project configuration
    pub project_config: Arc<ProjectConfig>,
    /// Database and persistence manager
    pub persistency_manager: Arc<PersistencyManager>,
    /// Network connectivity checker
    pub connectivity_checker: Arc<ConnectivityChecker>,
    /// OneDrive API client
    pub onedrive_client: Arc<OneDriveClient>,
    /// Authentication manager
    pub auth: Arc<OneDriveAuth>,
    /// File manager
    pub file_manager: Arc<DefaultFileManager>,
    /// Message broker for internal communication
    pub message_broker: Arc<MessageBroker>,
    /// Scheduler for periodic tasks
    pub scheduler: Arc<PeriodicScheduler>,
}

impl AppState {
    /// Create a new application state with all required components
    pub async fn new() -> Result<Self> {
        // Initialize project configuration
        let project_config = ProjectConfig::new()
            .await
            .context("Failed to create project configuration")?;
        let project_config_arc = Arc::new(project_config);

        // Initialize persistence manager
        let persistency_manager =
            PersistencyManager::new(project_config_arc.project_dirs.data_dir().to_path_buf())
                .await
                .context("Failed to create persistence manager")?;

        // Initialize connectivity checker
        let connectivity_checker = ConnectivityChecker::new();

        // Initialize authentication
        let auth = OneDriveAuth::new()
            .await
            .context("Failed to create authentication manager")?;
        let auth_arc = Arc::new(auth);

        // Initialize OneDrive client
        let onedrive_client =
            OneDriveClient::new(auth_arc.clone()).context("Failed to create OneDrive client")?;

        // Initialize file manager
        let file_manager = Arc::new(DefaultFileManager::new(project_config_arc.clone()).await?);

        // Initialize message broker
        let message_broker = Arc::new(MessageBroker::new(1000));

        // Initialize scheduler
        let scheduler = Arc::new(PeriodicScheduler::new());

        Ok(Self {
            project_config: project_config_arc,
            persistency_manager: Arc::new(persistency_manager),
            connectivity_checker: Arc::new(connectivity_checker),
            onedrive_client: Arc::new(onedrive_client),
            auth: auth_arc,
            file_manager,
            message_broker,
            scheduler,
        })
    }
    

    /// Get a reference to the project configuration
    pub fn config(&self) -> &ProjectConfig {
        &self.project_config
    }

    /// Get a reference to the persistence manager
    pub fn persistency(&self) -> &PersistencyManager {
        &self.persistency_manager
    }

    /// Get a reference to the connectivity checker
    pub fn connectivity(&self) -> &ConnectivityChecker {
        &self.connectivity_checker
    }

    /// Get a reference to the OneDrive client
    pub fn onedrive(&self) -> &OneDriveClient {
        &self.onedrive_client
    }

    /// Get a reference to the authentication manager
    pub fn auth(&self) -> &OneDriveAuth {
        &self.auth
    }

    /// Get a reference to the file manager
    pub fn file_manager(&self) -> &DefaultFileManager {
        &self.file_manager
    }

    /// Get a reference to the message broker
    pub fn message_broker(&self) -> &MessageBroker {
        &self.message_broker
    }

    /// Get a reference to the scheduler
    pub fn scheduler(&self) -> &PeriodicScheduler {
        &self.scheduler
    }
}

/// Factory function to create application state
///
/// This function initializes all required components and returns
/// a fully configured application state.
pub async fn app_state_factory() -> Result<AppState> {
    AppState::new().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_app_state_creation() {
        // This test would require mocking of dependencies
        // For now, we'll just test that the function signature is correct
        let _app_state = AppState::new().await;
        // In a real test, we would assert on the created state
    }

    #[tokio::test]
    async fn test_app_state_factory() {
        // This test would require mocking of dependencies
        let _app_state = app_state_factory().await;
        // In a real test, we would assert on the created state
    }
}
