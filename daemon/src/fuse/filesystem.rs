//! Main FUSE filesystem implementation

use crate::file_manager::DefaultFileManager;
use crate::fuse::attributes::AttributeManager;
use crate::fuse::utils::sync_await;
use crate::persistency::cached_drive_item_with_fuse_repository::CachedDriveItemWithFuseRepository;
use crate::persistency::download_queue_repository::DownloadQueueRepository;
use crate::persistency::drive_item_with_fuse_repository::DriveItemWithFuseRepository;
use crate::persistency::types::DriveItemWithFuse;
use anyhow::Result;
use log::{info, warn};
use sqlx::Pool;
use std::sync::Arc;
use std::path::{Path, PathBuf};
use std::io::{Read, Seek, SeekFrom, Write};
use crate::fuse::operations::MetadataToFileAttr;

use crate::fuse::database::DatabaseManager;
use crate::fuse::file_handles::FileHandleManager;


/// OneDrive FUSE filesystem implementation using DriveItemWithFuse
pub struct OneDriveFuse {
    drive_item_with_fuse_repo: Arc<CachedDriveItemWithFuseRepository>,
    file_manager: Arc<DefaultFileManager>,
    #[allow(dead_code)]
    app_state: Arc<crate::app_state::AppState>,

    // Managers for different responsibilities
    file_handle_manager: FileHandleManager,
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
        let drive_item_with_fuse_repo =
            Arc::new(CachedDriveItemWithFuseRepository::new_with_default_ttl(
                Arc::new(DriveItemWithFuseRepository::new(pool)),
            ));

        let file_handle_manager = FileHandleManager::new(file_manager.clone(), app_state.clone());



        let database_manager = DatabaseManager::new(drive_item_with_fuse_repo.clone());

        Ok(Self {
            drive_item_with_fuse_repo,
            file_manager,
            app_state,
            file_handle_manager,
            database_manager,
        })
    }

    /// Initialize the filesystem by ensuring root directory exists
    pub async fn initialize(&self) -> Result<()> {
        info!("Initializing OneDrive FUSE filesystem...");

        // Check if root directory exists in database
        let root_item = crate::fuse::utils::sync_await(
            self.drive_item_with_fuse_repo
                .get_drive_item_with_fuse_by_virtual_ino(1),
        )?;

        if root_item.is_none() {
            // Database not initialized - root should come from delta sync
            // For now, we'll create a temporary stub for FUSE operations
            // This stub is NOT stored in DB and will be replaced by real OneDrive root
            warn!("Root directory not found in database - using temporary stub. Run delta sync to populate real OneDrive data.");

            // Note: We don't store this stub in the database
            // The real root will be populated by delta sync process
        } else {
            info!(
                "Found root directory: {} (OneDrive ID: {})",
                root_item.as_ref().unwrap().name().unwrap_or("root"),
                root_item.as_ref().unwrap().id()
            );
        }

        info!("FUSE filesystem initialized successfully");
        Ok(())
    }

    // Delegate methods to appropriate managers

    /// Get file handle manager
    pub fn file_handles(&self) -> &FileHandleManager {
        &self.file_handle_manager
    }



    /// Get database manager
    pub fn database(&self) -> &DatabaseManager {
        &self.database_manager
    }

    /// Get drive item with fuse repository
    pub fn drive_item_with_fuse_repo(&self) -> &Arc<CachedDriveItemWithFuseRepository> {
        &self.drive_item_with_fuse_repo
    }

    /// Get file manager
    pub fn file_manager(&self) -> &Arc<DefaultFileManager> {
        &self.file_manager
    }

    /// Get app state
    #[allow(dead_code)]
    pub fn app_state(&self) -> &Arc<crate::app_state::AppState> {
        &self.app_state
    }
    pub fn add_dot_entries_if_needed(
        &self,
        ino: u64,
        reply: &mut fuser::ReplyDirectory,
        offset: i64,
    ) -> bool {
        if offset < 2 {
            // We need to add at least .
            let item = sync_await(self.database().get_item_by_ino(ino))
                .unwrap()
                .unwrap();
            let dot_ino = item.virtual_ino().unwrap_or(ino);
            let _r = reply.add(dot_ino, 1, fuser::FileType::Directory, ".".to_string());
            //Assuming tht buffer cannot get full so fast
            if offset == 1 {
                let dotdot_ino = item.parent_ino().unwrap_or(1);
                let _r = reply.add(dotdot_ino, 2, fuser::FileType::Directory, "..".to_string());
            }
            return true;
        }
        return false;
    }
    pub fn get_attributes_from_local_file_or_from_db(&self, item: &DriveItemWithFuse) -> fuser::FileAttr {
        if let Some(file_path) = self.get_local_file_path(item.virtual_ino().unwrap_or(0)) {
            let metadata = std::fs::metadata(&file_path).unwrap();
            return metadata.try_to_file_attr(item.virtual_ino().unwrap()).unwrap();
        }
        AttributeManager::item_to_file_attr(&item)
    }

    // Direct file operations (no wrapper)
    pub fn get_local_file_path(&self, ino: u64) -> Option<PathBuf> {
        self.file_manager.get_local_path_if_file_exists(ino)
    }
    
    pub fn is_file_synchronized(&self, item: &DriveItemWithFuse) -> bool {
        item.drive_item().id.starts_with("local_")
    }
    
    pub fn generate_placeholder_content(&self, item: &DriveItemWithFuse) -> Vec<u8> {
        let name = item.name().unwrap_or("unknown");
        let size = item.size();

        let placeholder = format!(
            "This is a placeholder for file: {}\nSize: {} bytes\nThis file is not yet downloaded locally.",
            name, size
        );

        placeholder.into_bytes()
    }

    // File I/O helper methods (extracted from operations)
    pub fn read_file_data(&self, path: &Path, offset: u64, size: usize) -> Result<Vec<u8>, std::io::Error> {
        let mut file = std::fs::OpenOptions::new().read(true).open(path)?;
        file.seek(std::io::SeekFrom::Start(offset))?;
        let mut buffer = vec![0; size];
        let bytes_read = file.read(&mut buffer)?;
        buffer.truncate(bytes_read);
        Ok(buffer)
    }

    pub fn write_file_data(&self, path: &Path, offset: u64, data: &[u8], flags: i32) -> Result<u32, std::io::Error> {
        use std::io::{Seek, SeekFrom, Write};
        
        let mut open_options = std::fs::OpenOptions::new();
        open_options.write(true);
        
        // Parse flags for append mode
        if (flags & libc::O_APPEND) != 0 {
            open_options.append(true);
        }
        
        let mut file = open_options.open(path)?;
        
        if (flags & libc::O_APPEND) != 0 {
            // Seek to end for append mode
            file.seek(SeekFrom::End(0))?;
        } else {
            // Seek to specified offset
            file.seek(SeekFrom::Start(offset))?;
        }
        
        file.write_all(data)?;
        Ok(data.len() as u32)
    }
}
