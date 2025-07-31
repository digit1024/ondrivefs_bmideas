//! File handle management for FUSE filesystem

use crate::persistency::processing_item_repository::{ProcessingItem, ProcessingItemRepository};
use crate::file_manager::DefaultFileManager;
use anyhow::Result;
use log::{debug, error};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use crate::fuse::utils::sync_await;

const VIRTUAL_FILE_HANDLE_ID: u64 = 1;

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

    /// Get or create a file handle for the given inode
    pub fn get_or_create_file_handle(&self, ino: u64) -> Result<u64> {
        let mut handles = self.open_handles.lock().unwrap();
        let mut next_id = self.next_handle_id.lock().unwrap();
        
        // Check if file is already open for this inode
        for (handle_id, handle) in handles.iter() {
            if handle.ino == ino {
                debug!("📂 Reusing existing file handle {} for inode {}", 
                       handle_id, ino);
                return Ok(*handle_id);
            }
        }
        
        // Check if this is a virtual file (no local file exists)
        if self.file_manager.get_local_path_if_file_exists(ino).is_none() {
            // Get item from database to check if it's a folder
            if let Ok(Some(item)) = sync_await(self.get_item_by_ino(ino)) {
                // Only create virtual handles for files, not folders
                if !item.is_folder() {
                    debug!("📂 Creating virtual file handle {} for inode {} (no local file)", 
                           VIRTUAL_FILE_HANDLE_ID, ino);
                    return Ok(VIRTUAL_FILE_HANDLE_ID);
                }
            }
        }
        
        // Get local file path for real files
        let local_path = self.file_manager.get_local_path_if_file_exists(ino)
            .ok_or_else(|| anyhow::anyhow!("File not found in local folder for inode: {}", ino))?;
        
        // Create new file handle
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&local_path)?;
        
        let handle_id = *next_id;
        *next_id += 1;
        
        // Get OneDrive ID from database for the handle (needed for sync operations)
        let onedrive_id = sync_await(self.get_onedrive_id_for_inode(ino))
            .unwrap_or_else(|_| format!("unknown_{}", ino));
        
        let open_handle = OpenFileHandle {
            file,
            onedrive_id,
            ino,
            is_dirty: false,
        };
        
        handles.insert(handle_id, open_handle);
        debug!("📂 Created new file handle {} for inode {} at {}", 
               handle_id, ino, local_path.display());
        
        Ok(handle_id)
    }

    /// Close a file handle and clean up resources
    pub fn close_file_handle(&self, fh: u64) -> Result<()> {
        let mut handles = self.open_handles.lock().unwrap();
        
        if let Some(handle) = handles.get(&fh) {
            if handle.is_dirty {
                // Create ProcessingItem for the dirty file
                if let Err(e) = sync_await(self.create_processing_item_for_handle(&handle.onedrive_id)) {
                    error!("Failed to create ProcessingItem for dirty handle {}: {}", fh, e);
                }
            }
        }
        
        // Close the file and remove from cache
        if let Some(handle) = handles.remove(&fh) {
            drop(handle.file); // Explicitly close the file
            debug!("📂 Closed and removed file handle {} for {}", fh, handle.onedrive_id);
        }
        
        Ok(())
    }

    /// Read data from a file handle
    pub fn read_from_handle(&self, fh: u64, offset: u64, size: u32) -> Result<Vec<u8>> {
        let mut handles = self.open_handles.lock().unwrap();
        
        if let Some(handle) = handles.get_mut(&fh) {
            handle.file.seek(SeekFrom::Start(offset))?;
            let mut buffer = vec![0u8; size as usize];
            let bytes_read = handle.file.read(&mut buffer)?;
            buffer.truncate(bytes_read);
            return Ok(buffer);
        }
        
        Err(anyhow::anyhow!("File handle {} not found", fh))
    }

    /// Write data to a file handle
    pub fn write_to_handle(&self, fh: u64, offset: u64, data: &[u8]) -> Result<()> {
        let mut handles = self.open_handles.lock().unwrap();
        
        if let Some(handle) = handles.get_mut(&fh) {
            handle.file.seek(SeekFrom::Start(offset))?;
            handle.file.write_all(data)?;
            handle.file.sync_data()?; // Ensure data is written to disk
            handle.is_dirty = true; // Mark as dirty
            return Ok(());
        }
        
        Err(anyhow::anyhow!("File handle {} not found", fh))
    }

    /// Clean up all handles for a specific inode
    pub fn cleanup_handles_for_inode(&self, ino: u64) {
        let mut handles = self.open_handles.lock().unwrap();
        let keys_to_remove: Vec<u64> = handles.iter()
            .filter(|(_, handle)| handle.ino == ino)
            .map(|(key, _)| *key)
            .collect();
        
        for key in keys_to_remove {
            if let Some(handle) = handles.remove(&key) {
                debug!("📂 Cleaned up file handle {} for inode {} ({})", key, ino, handle.onedrive_id);
                drop(handle.file);
            }
        }
    }

    /// Get OneDrive ID for a given inode from database
    async fn get_onedrive_id_for_inode(&self, ino: u64) -> Result<String> {
        if let Ok(Some(item)) = sync_await(self.app_state.persistency().drive_item_with_fuse_repository().get_drive_item_with_fuse_by_virtual_ino(ino)) {
            Ok(item.id().to_string())
        } else {
            Err(anyhow::anyhow!("No item found for inode: {}", ino))
        }
    }

    /// Get item by inode from database
    async fn get_item_by_ino(&self, ino: u64) -> Result<Option<crate::persistency::types::DriveItemWithFuse>> {
        self.app_state.persistency().drive_item_with_fuse_repository().get_drive_item_with_fuse_by_virtual_ino(ino).await
    }

    /// Create a processing item for a dirty handle
    async fn create_processing_item_for_handle(&self, onedrive_id: &str) -> Result<()> {
        // Get the item from database
        if let Ok(Some(item)) = sync_await(self.app_state.persistency().drive_item_with_fuse_repository().get_drive_item_with_fuse(&onedrive_id)) {
            //if onedriveId is local then it's create 
            let operation  = if onedrive_id.starts_with("local_") {
                crate::persistency::processing_item_repository::ChangeOperation::Create
            } else {
                crate::persistency::processing_item_repository::ChangeOperation::Update
            };
            
            let processing_item = ProcessingItem::new_local(
                item.drive_item().clone(),
                operation
            );
            
            let processing_repo = ProcessingItemRepository::new(
                self.app_state.persistency().pool().clone()
            );
            let _id = sync_await(processing_repo.store_processing_item(&processing_item))?;
            debug!("📝 Created ProcessingItem for dirty handle: {} (DB ID: {})", onedrive_id, _id);
        }
        Ok(())
    }
} 