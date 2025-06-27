//! SyncService: Handles all OneDrive <-> local sync logic.

use crate::onedrive_service::onedrive_client::OneDriveClient;
use crate::config::{Settings, SyncConfig};
use crate::metadata_manager_for_files::OnedriveFileMeta;
use anyhow::Result;
use std::path::PathBuf;
use tokio::time::sleep;
use tokio::signal;
use log::{info, error, warn};
use std::str::FromStr;


/// Service responsible for synchronizing files between OneDrive and local storage.
pub struct SyncService {
    pub client: OneDriveClient,
    pub config: SyncConfig,
    pub settings: Settings,
}

impl SyncService {
    pub fn new(client: OneDriveClient, config: SyncConfig, settings: Settings) -> Self {
        Self { client, config, settings }
    }

    /// Sync files from OneDrive to local directory using delta queries
    pub async fn sync_from_remote(&mut self) -> Result<()> {
        info!("Starting sync from remote using delta queries...");
        for folder in &self.settings.sync_folders {
            info!("Processing folder: {}", folder);
            let delta_token = self.client.metadata_manager().get_folder_delta(folder)?;
            let initial_sync = delta_token.is_none();
            if initial_sync {
                let local_folder = if folder == "/" {
                    self.config.local_dir.clone()
                } else {
                    let mut p = self.config.local_dir.clone();
                    p.push(folder.trim_start_matches('/'));
                    p
                };
                if local_folder.exists() {
                    for entry in std::fs::read_dir(&local_folder)? {
                        let entry = entry?;
                        let path = entry.path();
                        if path.is_file() {
                            std::fs::remove_file(&path)?;
                            info!("Removed local file on initial sync: {:?}", path);
                        }
                    }
                }
            }
            let changes = if let Some(delta) = delta_token {
                info!("Delta token found for folder: {}", folder);
                self.client.get_delta_with_token(folder, &delta.delta_token).await?
            } else {
                info!("Initial sync for folder: {}", folder);
                self.client.get_initial_delta(folder).await?
            };
            for item in &changes.value {
                // Create local directory if it doesn't exist
                if item.folder.is_some() && item.deleted.is_none() {
                    
                    let local_path = self.get_local_path_for_item(folder, item);
                    info!("Creating local directory: {:?}", local_path);
                    if !local_path.exists() {
                        if let Err(e) = tokio::fs::create_dir_all(&local_path).await {
                            error!("Failed to create directory {:?}: {}", local_path, e);
                        } else {
                            info!("Created local directory: {:?}", local_path);
                        }
                    }
                }

                
                if item.deleted.is_some() {
                    let stored_local_path = self.client.metadata_manager().get_local_path_from_one_drive_id(&item.id).unwrap().unwrap();
                    let local_path = PathBuf::from_str(&stored_local_path).unwrap();
                    if local_path.exists() {
                        if let Err(e) = std::fs::remove_file(&local_path) {
                            error!("Failed to delete local file {:?}: {}", local_path, e);
                        } else {
                            info!("Deleted local file: {:?}", local_path);
                        }
                    }
                } 
                if item.file.is_some() {
                    let local_path = self.get_local_path_for_item(folder, item);
                    let skip = if let Some(etag) = &item.etag {
                        if let Ok(Some(meta)) = self.client.metadata_manager().get_onedrive_file_meta(&local_path.to_string_lossy()) {
                            meta.etag == *etag
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    if skip {
                        info!("Skipping download for {:?} (etag matches)", local_path);
                        continue;
                    }
                    if let Some(parent) = local_path.parent() {
                        if let Err(e) = tokio::fs::create_dir_all(parent).await {
                            error!("Failed to create directory {:?}: {}", parent, e);
                            continue;
                        }
                    }
                    
                 
                    let remote_path_from_parent = item.parent_reference.as_ref().unwrap().path.as_ref().unwrap();
                    // Now I need to remove /drive/root: from the remote_path_from_parent
                    let remote_path_from_parent = remote_path_from_parent.trim_start_matches("/drive/root:");
                    
                    let remote_path = format!("{}/{}", remote_path_from_parent, item.name.as_ref().map_or("Unknown", |v| v));
                    
                    let local_root_path = self.get_local_root_path();
                    info!("local_root_path: {:?}", local_root_path);
                    let local_path = local_root_path.join(remote_path.clone().trim_start_matches("/"));
                    info!("local_path: {:?}", local_path);
                    info!("----------------------------------");

                    match self.client.get_item_by_path(&remote_path).await {
                        Ok(full_item) => {
                            if let Some(download_url) = &full_item.download_url {
                                let etag = full_item.etag.as_ref();
                                match self.client.download_file(download_url, &local_path, &item.id, item.name.as_ref().map_or("Unknown", |v| v), etag).await {
                                    Ok(_) => info!("Downloaded: {} -> {:?}", item.name.as_ref().map_or("Unknown", |v| v), local_path),
                                    Err(e) => error!("Failed to download {}: {}", item.name.as_ref().map_or("Unknown", |v| v), e),
                                }
                            } else {
                                error!("No download URL available for file: {}", item.name.as_ref().map_or("Unknown", |v| v));
                            }
                        }
                        Err(e) => {
                            error!("Failed to get download URL for {}: {}", item.name.as_ref().map_or("Unknown", |v| v), e);
                        }
                    }
                    if let Some(etag) = &item.etag {
                        let meta = OnedriveFileMeta {
                            etag: etag.clone(),
                            id: item.id.clone(),
                        };
                        self.client.metadata_manager().set_onedrive_file_meta(&local_path.to_string_lossy(), &meta)?;
                    }
                }
                
            }
            if let Some(delta_link) = &changes.delta_link {
                if let Some(token) = Self::extract_delta_token(delta_link) {
                    self.client.metadata_manager().store_folder_delta(folder, &token)?;
                    info!("Saved delta token for folder: {}", folder);
                }
            }
        }
        self.client.metadata_manager().flush()?;
        Ok(())
    }

    

    /// Run a single sync cycle
    pub async fn sync_cycle(&mut self) -> Result<()> {
        info!("Starting sync cycle");
        if let Err(e) = self.sync_from_remote().await {
            error!("Failed to sync from remote: {}", e);
        }
        // TODO: Implement local-to-remote sync
        // if let Err(e) = self.sync_to_remote().await {
        //     error!("Failed to sync to remote: {}", e);
        // }
        info!("Sync cycle completed");
        Ok(())
    }

    /// Helper: Get local path for a OneDrive item
    pub fn get_local_path_for_item(&self, folder: &str, item: &crate::onedrive_service::onedrive_models::DriveItem) -> PathBuf {
        let mut local_path = self.config.local_dir.clone();
        if folder != "/" {
            let folder_path = folder.trim_start_matches('/');
            local_path.push(folder_path);
        }
        if item.name.is_none() {
            panic!("Item name is missing for item with ID: {}", item.id)
        } else {
            local_path.push(item.name.as_ref().unwrap().clone());
        }
        local_path
    }
    pub fn get_local_root_path(&self) -> PathBuf {
        let local_path = self.config.local_dir.clone();
    
        local_path
    }

    pub async fn ensure_authorized(&self) -> Result<()> {
        match self.client.list_root().await {
            Ok(_) => {
                info!("Already authorized and tokens are valid");
                Ok(())
            }
            Err(_) => {
                warn!("Authorization needed or tokens expired");
                let auth = crate::auth::onedrive_auth::OneDriveAuth::new()?;
                auth.get_valid_token().await?;
                info!("Authorization completed");
                Ok(())
            }
        }
    }

    pub async fn run_daemon(&mut self) -> Result<()> {
        info!("Starting OneDrive sync daemon");
        info!("Local directory: {:?}", self.config.local_dir);
        info!("Sync folders: {:?}", self.settings.sync_folders);
        info!("Sync interval: {:?}", self.config.sync_interval);
        info!("Press Ctrl+C or send SIGTERM to stop the daemon gracefully");

        self.ensure_authorized().await?;

        loop {
            tokio::select! {
                _ = Self::wait_for_shutdown() => {
                    info!("Shutting down gracefully...");
                    break;
                }
                _ = async {
                    if let Err(e) = self.sync_cycle().await {
                        error!("Sync cycle failed: {}", e);
                    }
                    sleep(self.config.sync_interval).await;
                } => {}
            }
        }
        info!("Daemon stopped gracefully");
        Ok(())
    }

    async fn wait_for_shutdown() {
        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("Received Ctrl+C");
            }
            _ = async {
                let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
                    .expect("Failed to create SIGTERM signal handler");
                sigterm.recv().await;
            } => {
                info!("Received SIGTERM");
            }
        }
    }

    pub fn extract_delta_token(delta_link: &str) -> Option<String> {
        if let Some(token_start) = delta_link.find("token=") {
            let token = &delta_link[token_start + 6..];
            Some(token.to_string())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    #[tokio::test]
    #[serial]
    async fn test_sync_service_skeleton() {
        // TODO: Setup a mock OneDriveClient, config, and settings
        // let client = ...;
        // let config = ...;
        // let settings = ...;
        // let mut service = SyncService::new(client, config, settings);
        // assert!(service.sync_from_remote().await.is_ok());
        assert!(true); // Placeholder
    }
} 