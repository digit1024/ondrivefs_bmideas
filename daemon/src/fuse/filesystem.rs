//! Main FUSE filesystem implementation

use crate::file_manager::{DefaultFileManager, FileManager};
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

        let file_handle_manager = FileHandleManager::new();



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
        if offset < 1 {
            // We need to add at least .
            let item = sync_await(self.database().get_item_by_ino(ino))
                .unwrap()
                .unwrap();
            let dot_ino = item.virtual_ino().unwrap_or(ino);
            let _r = reply.add(dot_ino, 1, fuser::FileType::Directory, ".".to_string());
            if offset < 2 {
            //Assuming tht buffer cannot get full so fast
            
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



    // Enhanced write method that handles complex flag parsing and file operations
    pub fn write_file_with_flags(&self, path: &Path, offset: i64, data: &[u8], flags: i32) -> Result<u32, std::io::Error> {
        use std::io::{Seek, SeekFrom, Write};
        
        // Parse flags to understand how the file was opened
        let open_flags = self.parse_open_flags(flags)?;
        
        // Handle O_APPEND mode - ignore provided offset and seek to end
        let actual_offset = if open_flags.append {
            // In append mode, we need to get the current file size
            match std::fs::metadata(path) {
                Ok(metadata) => metadata.len() as i64,
                Err(e) => return Err(std::io::Error::new(std::io::ErrorKind::Other, e)),
            }
        } else {
            offset // Use the provided offset
        };
        
        // Configure open options based on the flags
        let mut open_options = std::fs::OpenOptions::new();
        self.apply_open_flags(&mut open_options, &open_flags);
        
        // For direct writes, we need special handling:
        // - If writing anywhere but the end, we need read access to preserve existing data
        // - If it's a new file (offset == 0), we can write directly
        if actual_offset > 0 && !open_flags.append {
            // We're writing in the middle of the file, need read access to preserve data
            open_options.read(true);
        }
        
        // Open the file and perform the write
        let mut file = open_options.open(path)?;
        
        // Seek to the correct position
        file.seek(SeekFrom::Start(actual_offset as u64))?;
        
        // Perform the actual write
        file.write_all(data)?;
        
        // Optional: Flush to ensure data is on disk
        if open_flags.write && (flags & libc::O_SYNC) != 0 {
            if let Err(e) = file.sync_all() {
                eprintln!("Warning: sync failed after O_SYNC write: {}", e);
            }
        }
        
        Ok(data.len() as u32)
    }

    // Parse open flags into a structured format
    fn parse_open_flags(&self, flags: i32) -> Result<OpenFlags, std::io::Error> {
        let mut config = OpenFlags::default();
        
        let access_mode = flags & libc::O_ACCMODE;
        match access_mode {
            libc::O_RDONLY => config.read = true,
            libc::O_WRONLY => config.write = true,
            libc::O_RDWR => {
                config.read = true;
                config.write = true;
            },
            _ => return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid access mode")),
        }
        
        // Set other flags
        config.append = (flags & libc::O_APPEND) != 0;
        config.create = (flags & libc::O_CREAT) != 0;
        config.truncate = (flags & libc::O_TRUNC) != 0;
        config.create_new = (flags & libc::O_EXCL) != 0;
        
        Ok(config)
    }

    // Apply open flags to OpenOptions
    fn apply_open_flags<'a>(&self, options: &'a mut std::fs::OpenOptions, flags: &OpenFlags) -> &'a mut std::fs::OpenOptions {
        options
            .read(flags.read)    
            .write(flags.write)
            .append(flags.append)
            .create(flags.create)
            .truncate(flags.truncate)
            .create_new(flags.create_new)
    }

    // Create physical file with flags and return file handle and attributes
    pub fn create_physical_file(&self, file_path: &Path, flags: i32) -> Result<(std::fs::File, fuser::FileAttr), std::io::Error> {
        use std::io::{Seek, SeekFrom};
        
        // Parse flags
        let append = (flags & libc::O_APPEND) != 0;
        let truncate = (flags & libc::O_TRUNC) != 0;
        
        let mut open_options = std::fs::OpenOptions::new();
        open_options.write(true).create(true);
        if append {
            open_options.append(true);
        }
        if truncate {
            open_options.truncate(true);
        }
        
        let mut file = open_options.open(file_path)?;
        
        // Handle O_TRUNC - if file existed and we're truncating
        if truncate && file_path.exists() {
            if let Err(e) = file.set_len(0) {
                eprintln!("Warning: failed to truncate file: {}", e);
            }
        }

        // For O_APPEND, seek to end
        if append {
            if let Ok(metadata) = std::fs::metadata(file_path) {
                let seek_pos = metadata.len();
                if let Err(e) = file.seek(SeekFrom::Start(seek_pos)) {
                    eprintln!("Warning: failed to seek to end: {}", e);
                }
            }
        }

        // Get file attributes
        let metadata = std::fs::metadata(file_path)?;
        let attr = metadata.try_to_file_attr(0).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::Other, "Failed to convert metadata to file attributes")
        })?;

        Ok((file, attr))
    }

    // Check if file already exists (used in create operations)
    pub fn file_already_exists(&self, parent: u64, name: &str) -> bool {
        sync_await(
            self.drive_item_with_fuse_repo()
                .get_drive_item_with_fuse_by_parent_ino_and_name_case_insensitive(parent, name)
        ).map(|item| item.is_some()).unwrap_or(false)
    }

    // Handle replace operation during rename
    pub fn handle_replace_operation(&self, original_item: &DriveItemWithFuse, target_item: &DriveItemWithFuse) -> Result<(), std::io::Error> {
        let local_path = self.file_manager.get_local_dir();
        let local_path_from = local_path.join(original_item.virtual_ino().unwrap().to_string());
        let local_path_to = local_path.join(target_item.virtual_ino().unwrap().to_string());
        
        if !local_path_from.exists() || !local_path_to.exists() {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "Local files not found"));
        }
        
        // Move the file
        std::fs::rename(&local_path_from, &local_path_to)?;
        
        // Delete original item from database
        if let Err(e) = sync_await(self.drive_item_with_fuse_repo().delete_drive_item_with_fuse(original_item.drive_item().id.clone().as_str())) {
            warn!("Failed to delete original item from database: {}", e);
        }
        
        // Create processing item for delete operation
        let delete_processing_item = crate::persistency::processing_item_repository::ProcessingItem::new_local(
            original_item.drive_item().clone(),
            crate::sync::ChangeOperation::Delete
        );
        
        let processing_repo = self.app_state.persistency().processing_item_repository();
        if let Err(e) = sync_await(processing_repo.store_processing_item(&delete_processing_item)) {
            warn!("Failed to store processing item: {}", e);
        }
        
        Ok(())
    }

    // Rename item in database
    pub fn rename_item_in_db(&self, item: &DriveItemWithFuse, new_parent: u64, new_name: &str) -> Result<(), anyhow::Error> {
        let mut updated_item = item.clone();

        // Update the name
        updated_item.drive_item_mut().set_name(new_name.to_string());

        // Update parent reference if moving to different parent
        if item.parent_ino().unwrap_or(0) != new_parent {
            if let Ok(Some(new_parent_item)) = sync_await(self.database().get_item_by_ino(new_parent)) {
                let new_parent_ref = crate::onedrive_service::onedrive_models::ParentReference {
                    id: new_parent_item.id().to_string(),
                    path: new_parent_item
                        .virtual_path()
                        .map(|p| format!("/drive/root:{}", p)),
                };
                updated_item.drive_item_mut().set_parent_reference(new_parent_ref);
                updated_item.set_parent_ino(new_parent);
            }
        }

        // Mark as local change
        updated_item.set_file_source(crate::persistency::types::FileSource::Local);

        // Store the updated item
        sync_await(self.drive_item_with_fuse_repo().store_drive_item_with_fuse(&updated_item))?;

        Ok(())
    }

    // Generic method to create processing items for sync operations
    pub fn create_processing_item(&self, item: &DriveItemWithFuse, operation: crate::sync::ChangeOperation) -> Result<(), anyhow::Error> {
        let processing_item = crate::persistency::processing_item_repository::ProcessingItem::new_local(
            item.drive_item().clone(),
            operation
        );
        let processing_repo = self.app_state.persistency().processing_item_repository();
        sync_await(processing_repo.store_processing_item(&processing_item))?;
        Ok(())
    }
}

// OpenFlags struct for parsing file open flags
#[derive(Debug, Default)]
pub struct OpenFlags {
    pub read: bool,
    pub write: bool,
    pub append: bool,
    pub create: bool,
    pub truncate: bool,
    pub create_new: bool,
}
