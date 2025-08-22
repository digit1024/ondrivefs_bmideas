//! File handle management for FUSE filesystem

use crate::file_manager::DefaultFileManager;
use crate::fuse::utils::sync_await;
use crate::persistency::processing_item_repository::{ProcessingItem, ProcessingItemRepository};
use anyhow::{Context, Result};
use log::{debug, error};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub const VIRTUAL_FILE_HANDLE_ID: u64 = 1;

/// File handle for caching open files
#[derive(Debug)]
pub struct OpenFileHandle {
    pub file: File,
    pub onedrive_id: String,
    pub ino: u64,
    pub is_dirty: bool,
}

/// File handle manager for the FUSE filesystem
pub struct FileHandleManager {
    open_handles: Arc<Mutex<HashMap<u64, OpenFileHandle>>>,
    next_handle_id: Arc<Mutex<u64>>,
    file_manager: Arc<DefaultFileManager>,
    app_state: Arc<crate::app_state::AppState>,
}

impl FileHandleManager {
    pub fn new(
        file_manager: Arc<DefaultFileManager>,
        app_state: Arc<crate::app_state::AppState>,
    ) -> Self {
        Self {
            open_handles: Arc::new(Mutex::new(HashMap::new())),
            next_handle_id: Arc::new(Mutex::new(2)), // Start from 2, 1 is reserved for virtual files
            file_manager,
            app_state,
        }
    }


    /// Create a processing item for a dirty handle
    pub async fn create_processing_item_for_handle(&self, onedrive_id: &str) -> Result<()> {
        // Get the item from database
        if let Ok(Some(item)) = sync_await(
            self.app_state
                .persistency()
                .drive_item_with_fuse_repository()
                .get_drive_item_with_fuse(&onedrive_id),
        ) {
            let processing_repo =
                ProcessingItemRepository::new(self.app_state.persistency().pool().clone());

            // Check if a pending processing item already exists for this drive item
            let existing = sync_await(processing_repo
                .get_pending_processing_item_by_drive_item_id_and_change_type(
                    &item.drive_item().id, 
                    &crate::persistency::processing_item_repository::ChangeType::Local
                ))?;

            if existing.is_some() {
                debug!(
                    "📝 Processing item already exists for {}, skipping duplicate creation",
                    onedrive_id
                );
                return Ok(());
            }

            //if onedriveId is local then it's create
            let operation = if onedrive_id.starts_with("local_") {
                crate::persistency::processing_item_repository::ChangeOperation::Create
            } else {
                crate::persistency::processing_item_repository::ChangeOperation::Update
            };

            let processing_item = ProcessingItem::new_local(item.drive_item().clone(), operation);

            let _id = sync_await(processing_repo.store_processing_item(&processing_item))?;
            debug!(
                "📝 Created ProcessingItem for dirty handle: {} (DB ID: {})",
                onedrive_id, _id
            );
        }
        Ok(())
    }
}
