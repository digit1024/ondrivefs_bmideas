use std::sync::Arc;

use onedrive_sync_lib::dbus::types::{DaemonStatus, SyncQueueItem, SyncStatus};
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
        let is_mounted = std::path::Path::new(&format!("{}/OneDrive", std::env::var("HOME").unwrap_or_default())).exists();
        
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
        
        let download_queue_repo = self.app_state.persistency().download_queue_repository();
        let items = download_queue_repo.get_all_items().await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to get download queue: {}", e)))?;
        
        let sync_items: Vec<SyncQueueItem> = items.into_iter()
            .map(|item| SyncQueueItem {
                onedrive_id: item.onedrive_id,
                ino: item.ino,
                name: item.name,
                path: item.virtual_path.unwrap_or_default(),
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

    async fn full_reset(&self) -> zbus::fdo::Result<()> {
        info!("DBus: full_reset called");
        
        // Clear all queues and processing items
        let processing_repo = self.app_state.persistency().processing_item_repository();
        let download_queue_repo = self.app_state.persistency().download_queue_repository();
        let sync_state_repo = self.app_state.persistency().sync_state_repository();

        
        processing_repo.clear_all_items().await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to clear processing items: {}", e)))?;
        sync_state_repo.clear_all_items().await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to clear sync state: {}", e)))?;
        download_queue_repo.clear_all_items().await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to clear download queue: {}", e)))?;
        
        
        info!("DBus: full_reset completed successfully");
        Ok(())
    }
}