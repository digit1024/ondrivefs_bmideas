use std::{fs, sync::Arc};

use onedrive_sync_lib::dbus::types::{DaemonStatus, SyncQueueItem, SyncStatus, UserProfile};
use zbus::interface;
use log::{info, error, debug};

use crate::persistency::sync_state_repository;

pub struct ServiceImpl {
    app_state: Arc<crate::app_state::AppState>,
}

impl ServiceImpl {
    pub fn new(app_state: Arc<crate::app_state::AppState>) -> Self {
        Self {
            app_state,
        }
    }
}

#[interface(name = "org.freedesktop.OneDriveSync")]
impl ServiceImpl {
    async fn get_user_profile(&self) -> zbus::fdo::Result<UserProfile> {
        debug!("DBus: get_user_profile called");
        
        let user_profile_repo = self.app_state.persistency().user_profile_repository();
        let user_profile = user_profile_repo.get_profile().await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to get user profile: {}", e)))?;
        let user_profile = user_profile.unwrap();
        Ok(UserProfile {
            display_name: user_profile.display_name.unwrap_or_default(),
            given_name: user_profile.given_name.unwrap_or_default(),
            mail: user_profile.mail.unwrap_or_default(),
        })
    }
    async fn get_daemon_status(&self) -> zbus::fdo::Result<DaemonStatus> {
        debug!("DBus: get_daemon_status called");
        
        // Get actual status from app state
        let is_authenticated = self.app_state.auth().get_valid_token().await.is_ok();
        let is_connected = matches!(
            self.app_state.connectivity().check_connectivity().await,
            crate::connectivity::ConnectivityStatus::Online
        );
        
        // Get sync status from scheduler
        let sync_status = if let Some(metrics) = self.app_state.scheduler().get_task_metrics("sync_cycle").await {
            if metrics.is_running {
                SyncStatus::Running
            } else {
                SyncStatus::Paused
            }
        } else {
            SyncStatus::Paused
        };
        
        // Check for conflicts
        let has_conflicts = self.app_state.persistency()
            .processing_item_repository()
            .get_processing_items_by_status(&crate::persistency::processing_item_repository::ProcessingStatus::Conflicted)
            .await
            .map(|items| !items.is_empty())
            .unwrap_or(false);
        
        // Check if FUSE is mounted
        let path_str = format!("{}/OneDrive", std::env::var("HOME").unwrap_or_default());
        let p = std::path::Path::new(&path_str);
        
            let mounts = fs::read_to_string("/proc/mounts").unwrap_or_default();
            let is_mounted =mounts.lines().any(|line| {
                line.split_whitespace().nth(1) == Some(p.to_str().unwrap_or_default())
            });
        


         
        
        Ok(DaemonStatus {
            is_authenticated,
            is_connected,
            sync_status,
            has_conflicts,
            is_mounted,
        })
    }

    async fn get_download_queue(&self) -> zbus::fdo::Result<Vec<SyncQueueItem>> {
        debug!("DBus: get_download_queue called");
        
        let drive_item_with_fuse_repo = self.app_state.persistency().drive_item_with_fuse_repository();

        let items = drive_item_with_fuse_repo.get_drive_items_with_fuse_in_download_queue().await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to get download queue: {}", e)))?;
        
        let sync_items: Vec<SyncQueueItem> = items.into_iter()
            .map(|item| SyncQueueItem {
                onedrive_id: item.drive_item.id.clone(),
                ino: item.fuse_metadata.virtual_ino.unwrap_or(0),
                name: item.drive_item.name.clone().unwrap_or_default(),
                path: item.drive_item.parent_reference.unwrap().path.clone().unwrap_or_default().replace("/drive/root:", "").to_string(),
            })
            .collect();
        
        Ok(sync_items)
    }

    async fn get_upload_queue(&self) -> zbus::fdo::Result<Vec<SyncQueueItem>> {
        debug!("DBus: get_upload_queue called");
        
        let processing_repo = self.app_state.persistency().processing_item_repository();
        let items = processing_repo.get_unprocessed_items_by_change_type(&crate::persistency::processing_item_repository::ChangeType::Local).await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to get upload queue: {}", e)))?;
        
        let sync_items: Vec<SyncQueueItem> = items.into_iter()
            .map(|item| SyncQueueItem {
                onedrive_id: item.drive_item.id,
                ino: item.id.unwrap_or(0) as u64,
                name: item.drive_item.name.unwrap_or_default(),
                path: item.drive_item.parent_reference.as_ref()
                    .and_then(|p| p.path.clone())
                    .unwrap_or_default(),
            })
            .collect();
        
        Ok(sync_items)
    }

    async fn get_recent_logs(&self) -> zbus::fdo::Result<Vec<String>> {
        use std::fs::File;
        use std::io::{BufRead, BufReader};
        use std::path::PathBuf;

        // Find the log file path
        let log_path: PathBuf = self.app_state.config().project_dirs.data_dir().join("logs/daemon.log");
        let file = File::open(&log_path)
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to open log file: {}", e)))?;
        let reader = BufReader::new(file);
        let lines: Vec<String> = reader.lines().filter_map(Result::ok).collect();
        let total = lines.len();
        let start = if total > 50 { total - 50 } else { 0 };
        Ok(lines[start..].to_vec())
    }

    async fn full_reset(&self) -> zbus::fdo::Result<()> {
        use std::fs;
        use std::process;
        use log::info;

        info!("DBus: full_reset called");

        // Clear all queues and processing items (existing logic)
        let processing_repo = self.app_state.persistency().processing_item_repository();
        let download_queue_repo = self.app_state.persistency().download_queue_repository();
        let sync_state_repo = self.app_state.persistency().sync_state_repository();
        let profile_repo = self.app_state.persistency().user_profile_repository();

        processing_repo.clear_all_items().await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to clear processing items: {}", e)))?;
        sync_state_repo.clear_all_items().await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to clear sync state: {}", e)))?;
        download_queue_repo.clear_all_items().await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to clear download queue: {}", e)))?;
        profile_repo.clear_profile().await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to clear profile: {}", e)))?;

        // Delete SQLite DB and token file
        
        let token_path = self.app_state.config().project_dirs.config_dir().join("secrets.json");
        
        let _ = fs::remove_file(&token_path);

        info!("DBus: full_reset deleted DB and token, exiting for restart.");
        panic!("Full reset requested");
        Ok(())
    }
    
}