use crate::persistency::processing_item_repository::{ChangeType, ProcessingStatus};
use onedrive_sync_lib::dbus::types::{ConflictItem, UserChoice};
use std::{fs, sync::Arc};

use log::{debug, error, info};
use onedrive_sync_lib::dbus::types::{DaemonStatus, SyncQueueItem, SyncStatus, UserProfile};
use onedrive_sync_lib::dbus::types::MediaItem;
use zbus::interface;
use zbus::object_server::SignalEmitter;

use crate::file_manager::FileManager;

pub struct ServiceImpl {
    app_state: Arc<crate::app_state::AppState>,
}

impl ServiceImpl {
    pub fn new(app_state: Arc<crate::app_state::AppState>) -> Self {
        Self { app_state }
    }
    pub async fn emit_daemon_status_changed(
        emitter: &SignalEmitter<'_>,
        status: DaemonStatus,
    ) -> zbus::Result<()> {
        Self::daemon_status_changed(emitter, status).await
    }
}

#[interface(name = "org.freedesktop.OneDriveSync")]
impl ServiceImpl {
    #[zbus(signal)]
    pub async fn daemon_status_changed(
        _emitter: &SignalEmitter<'_>,
        _status: DaemonStatus,
    ) -> zbus::Result<()> {
        Ok(())
    }
    

    async fn get_conflicts(&self) -> zbus::fdo::Result<Vec<ConflictItem>> {
        debug!("DBus: get_conflicts called");
        let processing_repo = self.app_state.persistency().processing_item_repository();
        let conflicted_items = processing_repo
            .get_processing_items_by_status(&ProcessingStatus::Conflicted)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to get conflicts: {}", e)))?;

        let mut conflict_list = Vec::new();
        for item in conflicted_items {
            let error_message = item.validation_errors.join(", ");
            conflict_list.push(ConflictItem {
                db_id: item.id.unwrap_or(0),
                onedrive_id: item.drive_item.id.clone(),
                name: item.drive_item.name.clone().unwrap_or_default(),
                path: item
                    .drive_item
                    .parent_reference
                    .as_ref()
                    .and_then(|p| p.path.clone())
                    .unwrap_or_default(),
                error_message,
                change_type: item.change_type.as_str().to_string(),
            });
        }
        Ok(conflict_list)
    }

    async fn resolve_conflict(
        &self,
        conflicted_item_db_id: i64,
        choice: UserChoice,
    ) -> zbus::fdo::Result<()> {
        debug!(
            "DBus: resolve_conflict called for item {} with choice {:?}",
            conflicted_item_db_id, choice
        );
        let processing_repo = self.app_state.persistency().processing_item_repository();

        let conflicted_item = match processing_repo
            .get_processing_item_by_id(conflicted_item_db_id)
            .await
        {
            Ok(Some(item)) => item,
            _ => {
                return Err(zbus::fdo::Error::Failed(
                    "Conflicted item not found".to_string(),
                ))
            }
        };

        let opposite_change_type = match conflicted_item.change_type {
            ChangeType::Local => ChangeType::Remote,
            ChangeType::Remote => ChangeType::Local,
        };

        let corresponding_item = processing_repo
            .get_pending_processing_item_by_drive_item_id_and_change_type(
                &conflicted_item.drive_item.id,
                &opposite_change_type,
            )
            .await
            .map_err(|e| {
                zbus::fdo::Error::Failed(format!("Failed to find corresponding item: {}", e))
            })?
            .ok_or_else(|| {
                zbus::fdo::Error::Failed("Corresponding conflicted item not found".to_string())
            })?;

        let (winning_item, losing_item) = match (choice, &conflicted_item.change_type) {
            (UserChoice::KeepLocal, &ChangeType::Local) => (conflicted_item, corresponding_item),
            (UserChoice::KeepLocal, &ChangeType::Remote) => (corresponding_item, conflicted_item),
            (UserChoice::UseRemote, &ChangeType::Remote) => (conflicted_item, corresponding_item),
            (UserChoice::UseRemote, &ChangeType::Local) => (corresponding_item, conflicted_item),
        };

        // Cancel the losing item
        processing_repo
            .update_status_by_id(losing_item.id.unwrap(), &ProcessingStatus::Cancelled)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to cancel losing item: {}", e)))?;

        // Re-queue the winning item by setting its status to New
        processing_repo
            .update_status_by_id(winning_item.id.unwrap(), &ProcessingStatus::New)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to re-queue winning item: {}", e)))?;

        info!(
            "Conflict resolved for OneDrive item {}. Kept {} version.",
            winning_item.drive_item.id,
            winning_item.change_type.as_str()
        );

        Ok(())
    }
    async fn get_user_profile(&self) -> zbus::fdo::Result<UserProfile> {
        debug!("DBus: get_user_profile called");

        let user_profile_repo = self.app_state.persistency().user_profile_repository();
        let user_profile = user_profile_repo
            .get_profile()
            .await
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
        let sync_status = if let Some(metrics) = self
            .app_state
            .scheduler()
            .get_task_metrics("sync_cycle")
            .await
        {
            if metrics.is_running {
                SyncStatus::Running
            } else {
                SyncStatus::Paused
            }
        } else {
            SyncStatus::Paused
        };

        // Check for conflicts
        let has_conflicts = self
            .app_state
            .persistency()
            .processing_item_repository()
            .get_processing_items_by_status(
                &crate::persistency::processing_item_repository::ProcessingStatus::Conflicted,
            )
            .await
            .map(|items| !items.is_empty())
            .unwrap_or(false);

        // Check if FUSE is mounted
        let path_str = format!("{}/OneDrive", std::env::var("HOME").unwrap_or_default());
        let p = std::path::Path::new(&path_str);

        let mounts = fs::read_to_string("/proc/mounts").unwrap_or_default();
        let is_mounted = mounts
            .lines()
            .any(|line| line.split_whitespace().nth(1) == Some(p.to_str().unwrap_or_default()));

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

        let drive_item_with_fuse_repo = self
            .app_state
            .persistency()
            .drive_item_with_fuse_repository();

        let items = drive_item_with_fuse_repo
            .get_drive_items_with_fuse_in_download_queue()
            .await
            .map_err(|e| {
                zbus::fdo::Error::Failed(format!("Failed to get download queue: {}", e))
            })?;

        let sync_items: Vec<SyncQueueItem> = items
            .into_iter()
            .map(|item| SyncQueueItem {
                onedrive_id: item.drive_item.id.clone(),
                ino: item.fuse_metadata.virtual_ino.unwrap_or(0),
                name: item.drive_item.name.clone().unwrap_or_default(),
                path: item
                    .drive_item
                    .parent_reference
                    .unwrap()
                    .path
                    .clone()
                    .unwrap_or_default()
                    .replace("/drive/root:", "")
                    .to_string(),
            })
            .collect();

        Ok(sync_items)
    }

    async fn get_upload_queue(&self) -> zbus::fdo::Result<Vec<SyncQueueItem>> {
        debug!("DBus: get_upload_queue called");

        let processing_repo = self.app_state.persistency().processing_item_repository();
        let items = processing_repo
            .get_unprocessed_items_by_change_type(
                &crate::persistency::processing_item_repository::ChangeType::Local,
            )
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to get upload queue: {}", e)))?;

        let sync_items: Vec<SyncQueueItem> = items
            .into_iter()
            .map(|item| SyncQueueItem {
                onedrive_id: item.drive_item.id,
                ino: item.id.unwrap_or(0) as u64,
                name: item.drive_item.name.unwrap_or_default(),
                path: item
                    .drive_item
                    .parent_reference
                    .as_ref()
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
        let log_path: PathBuf = self
            .app_state
            .config()
            .project_dirs
            .data_dir()
            .join("logs/daemon.log");
        let file = File::open(&log_path)
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to open log file: {}", e)))?;
        let reader = BufReader::new(file);
        let lines: Vec<String> = reader.lines().filter_map(Result::ok).collect();
        let total = lines.len();
        let start = if total > 50 { total - 50 } else { 0 };
        Ok(lines[start..].to_vec())
    }

    /// List media items (images/videos) newest first with pagination and optional date filters
    async fn list_media(&self, offset: u32, limit: u32, start_date: Option<String>, end_date: Option<String>) -> zbus::fdo::Result<Vec<MediaItem>> {
        let repo = self.app_state.persistency().drive_item_with_fuse_repository();
        let items = repo
            .get_media_items_paginated(start_date.as_deref(), end_date.as_deref(), offset as usize, limit as usize)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to list media: {}", e)))?;
        let mapped: Vec<MediaItem> = items
            .into_iter()
            .map(|it| MediaItem {
                onedrive_id: it.drive_item.id.clone(),
                ino: it.fuse_metadata.virtual_ino.unwrap_or(0),
                name: it.drive_item.name.unwrap_or_default(),
                virtual_path: it.fuse_metadata.virtual_path.unwrap_or_default(),
                mime_type: it.mime_type().map(|m| m.to_string()),
                created_date: it.drive_item.created_date.clone(),
                last_modified: it.drive_item.last_modified.clone(),
            })
            .collect();
        Ok(mapped)
    }

    /// Ensure a medium thumbnail exists for inode; returns absolute file path
    async fn fetch_thumbnail(&self, ino: u64) -> zbus::fdo::Result<String> {
        use tokio::fs;
        use tokio::io::AsyncWriteExt;
        let repo = self.app_state.persistency().drive_item_with_fuse_repository();
        let item = repo
            .get_drive_item_with_fuse_by_virtual_ino(ino)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to query item: {}", e)))?
            .ok_or_else(|| zbus::fdo::Error::Failed("Item not found".to_string()))?;
        let thumb_dir = self.app_state.config().thumbnails_dir();
        if !thumb_dir.exists() {
            std::fs::create_dir_all(&thumb_dir).map_err(|e| zbus::fdo::Error::Failed(format!("Failed to create thumbnails dir: {}", e)))?;
        }
        let path = thumb_dir.join(format!("{}.jpg", ino));
        if path.exists() {
            return Ok(path.to_string_lossy().to_string());
        }
        let bytes = self
            .app_state
            .onedrive()
            .download_thumbnail_medium(&item.drive_item.id)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to download thumbnail: {}", e)))?;
        let mut file = fs::File::create(&path).await.map_err(|e| zbus::fdo::Error::Failed(format!("Failed to create thumbnail file: {}", e)))?;
        file.write_all(&bytes).await.map_err(|e| zbus::fdo::Error::Failed(format!("Failed to write thumbnail: {}", e)))?;
        Ok(path.to_string_lossy().to_string())
    }

    async fn full_reset(&self) -> zbus::fdo::Result<()> {
        use log::info;
        use std::fs;
        use std::process;

        info!("DBus: full_reset called");

        // Clear all queues and processing items (existing logic)
        let processing_repo = self.app_state.persistency().processing_item_repository();
        let download_queue_repo = self.app_state.persistency().download_queue_repository();
        let sync_state_repo = self.app_state.persistency().sync_state_repository();
        let profile_repo = self.app_state.persistency().user_profile_repository();

        processing_repo.clear_all_items().await.map_err(|e| {
            zbus::fdo::Error::Failed(format!("Failed to clear processing items: {}", e))
        })?;
        sync_state_repo
            .clear_all_items()
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to clear sync state: {}", e)))?;
        download_queue_repo.clear_all_items().await.map_err(|e| {
            zbus::fdo::Error::Failed(format!("Failed to clear download queue: {}", e))
        })?;
        profile_repo
            .clear_profile()
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to clear profile: {}", e)))?;

        // Delete SQLite DB and token file

        let token_path = self
            .app_state
            .config()
            .project_dirs
            .config_dir()
            .join("secrets.json");

        let _ = fs::remove_file(&token_path);

        info!("DBus: full_reset deleted DB and token, exiting for restart.");
        panic!("Full reset requested");
    }

    /// List all sync folders
    async fn list_sync_folders(&self) -> zbus::fdo::Result<Vec<String>> {
        let folders = self
            .app_state
            .config()
            .settings
            .read()
            .await
            .download_folders
            .clone();
        Ok(folders)
    }

    /// Add a sync folder (store in settings, queue files for download)
    async fn add_sync_folder(&self, folder_path: String) -> zbus::fdo::Result<bool> {
        let mut settings = self.app_state.config().settings.read().await.clone();
        let normalized = folder_path.trim_start_matches('/').to_string();
        if settings.download_folders.contains(&normalized) {
            return Ok(false);
        }
        settings.download_folders.push(normalized.clone());
        // Save settings
        let config_path = self
            .app_state
            .config()
            .project_dirs
            .config_dir()
            .join("settings.json");
        if let Err(e) = settings.save_to_file(&config_path) {
            error!("Failed to save settings: {}", e);
            return Ok(false);
        }
        let mut settings_guard = self.app_state.config().settings.write().await;

        settings_guard.download_folders.push(normalized.clone());
        // Query files and queue for download
        let drive_item_with_fuse_repo = self
            .app_state
            .persistency()
            .drive_item_with_fuse_repository();
        let download_queue_repo = self.app_state.persistency().download_queue_repository();
        let file_manager = self.app_state.file_manager();
        match drive_item_with_fuse_repo
            .get_files_by_virtual_path_prefix(&normalized)
            .await
        {
            Ok(files) => {
                for file in files {
                    let local_path = file_manager.get_download_dir().join(&file.drive_item.id);
                    let _ = download_queue_repo
                        .add_to_download_queue(&file.drive_item.id, &local_path)
                        .await;
                }
            }
            Err(e) => {
                error!("Failed to query files for sync folder: {}", e);
                // Not critical, just continue
            }
        }
        Ok(true)
    }

    /// Remove a sync folder (remove from settings, delete downloaded files)
    async fn remove_sync_folder(&self, folder_path: String) -> zbus::fdo::Result<bool> {
        let mut settings = self.app_state.config().settings.read().await.clone();
        let normalized = folder_path.trim_start_matches('/').to_string();
        if !settings.download_folders.contains(&normalized) {
            return Ok(false);
        }
        settings.download_folders.retain(|f| f != &normalized);
        // Save settings
        let config_path = self
            .app_state
            .config()
            .project_dirs
            .config_dir()
            .join("settings.json");
        if let Err(e) = settings.save_to_file(&config_path) {
            error!("Failed to save settings: {}", e);
            return Ok(false);
        }
        //Update settings live
        let mut settings_guard = self.app_state.config().settings.write().await;

        settings_guard.download_folders.retain(|f| f != &normalized);

        // Query files and delete from downloads
        let drive_item_with_fuse_repo = self
            .app_state
            .persistency()
            .drive_item_with_fuse_repository();
        let file_manager = self.app_state.file_manager();

        match drive_item_with_fuse_repo
            .get_files_by_virtual_path_prefix(&normalized)
            .await
        {
            Ok(files) => {
                for file in files {
                    let local_path = file_manager.get_download_dir().join(&file.drive_item.id);
                    let _ = file_manager.delete_file(&local_path).await;
                }
            }
            Err(e) => {
                error!("Failed to query files for sync folder: {}", e);
                // Not critical, just continue
            }
        }
        Ok(true)
    }

    /// Toggle sync pause state
    async fn toggle_sync_pause(&self) -> zbus::fdo::Result<bool> {
        debug!("DBus: toggle_sync_pause called");
        
        let mut settings = self.app_state.config().settings.write().await;
        settings.sync_paused = !settings.sync_paused;
        
        // Save settings to file
        let config_path = self
            .app_state
            .config()
            .project_dirs
            .config_dir()
            .join("settings.json");
        
        if let Err(e) = settings.save_to_file(&config_path) {
            error!("Failed to save settings: {}", e);
            return Err(zbus::fdo::Error::Failed(format!("Failed to save settings: {}", e)));
        }
        
        let is_paused = settings.sync_paused;
        info!("Sync pause toggled: {}", if is_paused { "paused" } else { "resumed" });
        
        Ok(is_paused)
    }
}


