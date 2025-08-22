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

// VIRTUAL_FILE_HANDLE_ID is hardcoded as 1 in operations

/// File handle manager for the FUSE filesystem
pub struct FileHandleManager {
    files: Mutex<HashMap<u64, Arc<File>>>,
    next_id: Mutex<u64>,
    
    file_manager: Arc<DefaultFileManager>,
    app_state: Arc<crate::app_state::AppState>,
}

impl FileHandleManager {
    pub fn new(
        file_manager: Arc<DefaultFileManager>,
        app_state: Arc<crate::app_state::AppState>,
    ) -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
            next_id: Mutex::new(100000),
            file_manager,
            app_state,
        }
    }
    
    pub fn register_file(&self, file: File) -> u64 {
        
        let mut files = self.files.lock().unwrap();
        let mut next_id = self.next_id.lock().unwrap();
        
        let id = *next_id;
        *next_id += 1;
        files.insert(id, Arc::new(file));
        id
    }
    
    pub fn get_file(&self, fh: u64) -> Option<Arc<File>> {
        self.files.lock().unwrap().get(&fh).cloned()
    }
    
    pub fn close_file(&self, fh: u64) -> bool {
        self.files.lock().unwrap().remove(&fh).is_some()
    }



}
