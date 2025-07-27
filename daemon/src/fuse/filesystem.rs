//! Main FUSE filesystem implementation

use crate::persistency::drive_item_with_fuse_repository::DriveItemWithFuseRepository;
use crate::persistency::download_queue_repository::DownloadQueueRepository;
use crate::file_manager::DefaultFileManager;
use anyhow::Result;
use log::{info, warn};
use sqlx::Pool;
use std::sync::Arc;

use crate::fuse::file_handles::FileHandleManager;
use crate::fuse::file_operations::FileOperationsManager;
use crate::fuse::database::DatabaseManager;

/// OneDrive FUSE filesystem implementation using DriveItemWithFuse
pub struct OneDriveFuse {
    drive_item_with_fuse_repo: Arc<DriveItemWithFuseRepository>,
    file_manager: Arc<DefaultFileManager>,
    app_state: Arc<crate::app_state::AppState>,
    
    // Managers for different responsibilities
    file_handle_manager: FileHandleManager,
    file_operations_manager: FileOperationsManager,
    database_manager: DatabaseManager,
}

impl OneDriveFuse {
    /// Create a new OneDrive FUSE filesystem
    pub async fn new(
        pool: Pool<sqlx::Sqlite>, 
        download_queue_repo: DownloadQueueRepository, 
        file_manager: Arc<DefaultFileManager>,
        app_state: Arc<crate::app_state::AppState>,
    ) -> Result<Self> {
        let drive_item_with_fuse_repo = Arc::new(DriveItemWithFuseRepository::new(pool));
        
        let file_handle_manager = FileHandleManager::new(
            file_manager.clone(),
            app_state.clone(),
        );
        
        let file_operations_manager = FileOperationsManager::new(file_manager.clone());
        
        let database_manager = DatabaseManager::new(drive_item_with_fuse_repo.clone());
        
        Ok(Self {
            drive_item_with_fuse_repo,
            file_manager,
            app_state,
            file_handle_manager,
            file_operations_manager,
            database_manager,
        })
    }

    /// Initialize the filesystem by ensuring root directory exists
    pub async fn initialize(&self) -> Result<()> {
        info!("Initializing OneDrive FUSE filesystem...");

        // Check if root directory exists in database
        let root_item = crate::fuse::utils::sync_await(self.drive_item_with_fuse_repo.get_drive_item_with_fuse_by_virtual_ino(1))?;
        
        if root_item.is_none() {
            // Database not initialized - root should come from delta sync
            // For now, we'll create a temporary stub for FUSE operations
            // This stub is NOT stored in DB and will be replaced by real OneDrive root
            warn!("Root directory not found in database - using temporary stub. Run delta sync to populate real OneDrive data.");
            
            // Note: We don't store this stub in the database
            // The real root will be populated by delta sync process
        } else {
            info!("Found root directory: {} (OneDrive ID: {})", 
                  root_item.as_ref().unwrap().name().unwrap_or("root"),
                  root_item.as_ref().unwrap().id());
        }

        info!("FUSE filesystem initialized successfully");
        Ok(())
    }

    // Delegate methods to appropriate managers
    
    /// Get file handle manager
    pub fn file_handles(&self) -> &FileHandleManager {
        &self.file_handle_manager
    }

    /// Get file operations manager
    pub fn file_operations(&self) -> &FileOperationsManager {
        &self.file_operations_manager
    }

    /// Get database manager
    pub fn database(&self) -> &DatabaseManager {
        &self.database_manager
    }

    /// Get drive item with fuse repository
    pub fn drive_item_with_fuse_repo(&self) -> &Arc<DriveItemWithFuseRepository> {
        &self.drive_item_with_fuse_repo
    }

    /// Get file manager
    pub fn file_manager(&self) -> &Arc<DefaultFileManager> {
        &self.file_manager
    }

    /// Get app state
    pub fn app_state(&self) -> &Arc<crate::app_state::AppState> {
        &self.app_state
    }
} 