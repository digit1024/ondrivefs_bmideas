use std::sync::Arc;

use anyhow::Result;
use onedrive_sync_lib::config::ProjectConfig;

use crate::{
    auth::onedrive_auth::OneDriveAuth, connectivity::ConnectivityChecker,
    onedrive_service::onedrive_client::OneDriveClient, persistency::PersistencyManager,
};
// Mutable
#[derive(Clone)]
pub struct AppState {
    pub project_config: Arc<ProjectConfig>,
    pub persistency_manager: Arc<PersistencyManager>,
    pub connectivity_checker: Arc<ConnectivityChecker>,
    pub onedrive_client: Arc<OneDriveClient>,
    pub auth: Arc<OneDriveAuth>,
}

pub async fn app_state_factory() -> Result<AppState> {
    let project_config = ProjectConfig::new().await?;
    let persistency_manager =
        PersistencyManager::new(project_config.project_dirs.data_dir().to_path_buf()).await?;
    let connectivity_checker = ConnectivityChecker::new();
    let auth = OneDriveAuth::new().await?;
    let auth_arc = Arc::new(auth);

    let onedrive_client = OneDriveClient::new(auth_arc.clone())?;

    Ok(AppState {
        project_config: Arc::new(project_config),
        persistency_manager: Arc::new(persistency_manager),
        connectivity_checker: Arc::new(connectivity_checker),
        onedrive_client: Arc::new(onedrive_client),
        auth: auth_arc,
    })
}
