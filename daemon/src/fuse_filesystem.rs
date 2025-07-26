use crate::persistency::drive_item_with_fuse_repository::DriveItemWithFuseRepository;
use crate::persistency::download_queue_repository::DownloadQueueRepository;
use crate::persistency::types::{DriveItemWithFuse, FileSource};
use crate::file_manager::{FileManager, DefaultFileManager};
use crate::onedrive_service::onedrive_models::DriveItem;
use anyhow::Result;
use fuser::{
    FileAttr, FileType, KernelConfig, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory, ReplyEntry, ReplyStatfs, ReplyWrite
};
use libc::c_int;
use log::{debug, info, warn, error};
use sqlx::types::chrono;
use std::ffi::OsStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::runtime::Handle;
use std::path::PathBuf;
use sqlx::Pool;
const FUSE_CAP_READDIRPLUS: u32 = 0x00000010;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

/// OneDrive FUSE filesystem implementation using DriveItemWithFuse
pub struct OneDriveFuse {
    drive_item_with_fuse_repo: Arc<DriveItemWithFuseRepository>,
    file_manager: Arc<DefaultFileManager>,
    app_state: Arc<crate::app_state::AppState>,
}

fn sync_await<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    tokio::task::block_in_place(|| Handle::current().block_on(future))
}

impl OneDriveFuse {
    /// Create a new OneDrive FUSE filesystem
    pub async fn new(
        pool: Pool<sqlx::Sqlite>, 
        download_queue_repo: DownloadQueueRepository, 
        file_manager: Arc<DefaultFileManager>,
        app_state: Arc<crate::app_state::AppState>,
    ) -> Result<Self> {
        let drive_item_with_fuse_repo = DriveItemWithFuseRepository::new(pool);
        
        Ok(Self {
            drive_item_with_fuse_repo: Arc::new(drive_item_with_fuse_repo),
            file_manager,
            app_state,
        })
    }

    /// Initialize the filesystem by ensuring root directory exists
    pub async fn initialize(&self) -> Result<()> {
        info!("Initializing OneDrive FUSE filesystem...");

        // Check if root directory exists in database
        let root_item = sync_await(self.drive_item_with_fuse_repo.get_drive_item_with_fuse_by_virtual_ino(1))?;
        
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

    /// Create a temporary root stub for FUSE operations (not stored in DB)
    fn create_temp_root_stub(&self) -> DriveItemWithFuse {
        let root_drive_item = DriveItem {
            id: "temp_root".to_string(),
            name: Some("root".to_string()),
            etag: None,
            last_modified: None,
            created_date: None,
            size: Some(0),
            folder: Some(crate::onedrive_service::onedrive_models::FolderFacet { child_count: 0 }),
            file: None,
            download_url: None,
            deleted: None,
            parent_reference: None,
        };

        let mut root_with_fuse = self.drive_item_with_fuse_repo.create_from_drive_item(root_drive_item);
        root_with_fuse.set_virtual_ino(1);
        root_with_fuse.set_virtual_path("/".to_string());
        
        root_with_fuse.set_file_source(FileSource::Local); // Mark as local since it's a stub
        root_with_fuse.set_sync_status("stub".to_string());

        root_with_fuse
    }

    /// Get DriveItemWithFuse by inode
    async fn get_item_by_ino(&self, ino: u64) -> Result<Option<DriveItemWithFuse>> {
        let item = sync_await(self.drive_item_with_fuse_repo.get_drive_item_with_fuse_by_virtual_ino(ino))?;
        if item.is_some() {
            Ok(item)
        } else {
            if ino == 1 {
                Ok(Some(self.create_temp_root_stub()))
            } else {
                Ok(None)
            }
        }
        
        
    }



    /// Get DriveItemWithFuse by path
    async fn get_item_by_path(&self, path: &str) -> Result<Option<DriveItemWithFuse>> {
        let item = sync_await(self.drive_item_with_fuse_repo.get_drive_item_with_fuse_by_virtual_path(path));
        item
        
    }

    /// Get children of a directory by parent inode
    async fn get_children_by_parent_ino(&self, parent_ino: u64) -> Result<Vec<DriveItemWithFuse>> {
        sync_await(self.drive_item_with_fuse_repo.get_children_by_parent_ino(parent_ino))
    }

    /// Check if file exists locally with upload folder priority
    fn file_exists_locally(&self, onedrive_id: &str) -> Option<PathBuf> {
        self.file_manager.get_local_path_if_file_exists(onedrive_id)
    }

    /// Read data from a local staging folder
    fn read_local_file(&self, item: &DriveItemWithFuse, offset: u64, size: u32) -> Result<Vec<u8>> {
        let onedrive_id = item.id();
        
        // Get local file path with upload folder priority
        let local_path = self.file_exists_locally(onedrive_id)
            .ok_or_else(|| anyhow::anyhow!("Local file not found for OneDrive ID: {}", onedrive_id))?;

        // Read file data
        let mut file = std::fs::File::open(&local_path)?;
        file.seek(std::io::SeekFrom::Start(offset))?;
        let mut buffer = vec![0u8; size as usize];
        let bytes_read = file.read(&mut buffer)?;
        buffer.truncate(bytes_read);
        Ok(buffer)
    }

    /// Generate placeholder content for remote files with .onedrivedownload extension
    fn generate_placeholder_content(&self, item: &DriveItemWithFuse) -> Vec<u8> {
        let placeholder = serde_json::json!({
            "onedrive_id": item.id(),
            "name": item.name().unwrap_or("unknown"),
            "virtual_path": item.virtual_path().unwrap_or("unknown"),
            "mime_type": item.mime_type().unwrap_or("application/octet-stream"),
            "size": item.size(),
            "message": "This is a remote OneDrive file that has not been downloaded locally.",
            "instructions": "Double-click this file to download it from OneDrive.",
            "file_extension": ".onedrivedownload"
        });

        serde_json::to_string_pretty(&placeholder)
            .unwrap_or_else(|_| r#"{"error": "Failed to generate placeholder"}"#.to_string())
            .into_bytes()
    }

    /// Convert DriveItemWithFuse to FUSE FileAttr
    fn item_to_file_attr(&self, item: &DriveItemWithFuse) -> FileAttr {
        let now = SystemTime::now();
        let mtime = item.last_modified()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.into())
            .unwrap_or(now);

        FileAttr {
            ino: item.virtual_ino().unwrap_or(0),
            size: item.size(),
            blocks: (item.size() + 511) / 512, // 512-byte blocks
            atime: now,
            mtime,
            ctime: now,
            crtime: now,
            kind: if item.is_folder() {
                FileType::Directory
            } else {
                FileType::RegularFile
            },
            perm: if item.is_folder() { 0o755 } else { 0o644 },
            nlink: 1,
            uid: 1000, // TODO: Get from system
            gid: 1000, // TODO: Get from system
            rdev: 0,
            flags: 0,
            blksize: 512,
        }
    }

    /// Create a default FileAttr for new files
    fn create_default_attr(&self, ino: u64, is_folder: bool) -> FileAttr {
        FileAttr {
            ino,
            size: 0,
            blocks: 0,
            atime: SystemTime::now(),
            mtime: SystemTime::now(),
            ctime: SystemTime::now(),
            crtime: SystemTime::now(),
            kind: if is_folder {
                FileType::Directory
            } else {
                FileType::RegularFile
            },
            perm: if is_folder { 0o755 } else { 0o644 },
            nlink: 1,
            uid: 1000,
            gid: 1000,
            rdev: 0,
            flags: 0,
            blksize: 512,
        }
    }

    /// Generate a unique temporary ID for local changes
    fn generate_temporary_id(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        SystemTime::now().hash(&mut hasher);
        format!("local_{:x}", hasher.finish())
    }

    /// Apply local change directly to DriveItemWithFuse table
    async fn apply_local_change_to_db_repository(&self, change_type: &str, parent_ino: u64, name: &str, is_folder: bool) -> Result<u64> {
        let temporary_id = self.generate_temporary_id();
        
        // Get parent item to extract parent_id and parent_path
        let parent_item = sync_await(self.get_item_by_ino(parent_ino))?;
        let parent_id = parent_item.as_ref().map(|p| p.id().to_string());
        let parent_path = parent_item.as_ref().and_then(|p| p.virtual_path()).map(|p| format!("/drive/root:{}" , p.to_string()));
        
        // Create a new DriveItem for the local change
        let drive_item = DriveItem {
            id: temporary_id.clone(),
            name: Some(name.to_string()),
            etag: None,
            last_modified: Some(chrono::Utc::now().to_rfc3339()),
            created_date: Some(chrono::Utc::now().to_rfc3339()),
            size: Some(0),
            folder: if is_folder { 
                Some(crate::onedrive_service::onedrive_models::FolderFacet { child_count: 0 }) 
        } else {
                None 
            },
            file: if !is_folder { 
                Some(crate::onedrive_service::onedrive_models::FileFacet { mime_type: None }) 
            } else { 
                None 
            },
            download_url: None,
            deleted: None,
            parent_reference: parent_id.as_ref().map(|id| crate::onedrive_service::onedrive_models::ParentReference {
                id: id.clone(),
                path: parent_path.clone(),
            }),
        };

        let mut item_with_fuse = self.drive_item_with_fuse_repo.create_from_drive_item(drive_item.clone());
        item_with_fuse.set_parent_ino(parent_ino);
        item_with_fuse.set_file_source(FileSource::Local);
        item_with_fuse.set_sync_status("pending".to_string());
        
        
        
        
        // Set display path - convert raw OneDrive path to virtual path for display
        let display_path = if let Some(raw_parent_path) = &parent_path {
            // Convert raw OneDrive path to virtual path for display
            let virtual_parent_path = if raw_parent_path == "/drive/root:" {
                "/".to_string()
            } else {
                raw_parent_path.replace("/drive/root:", "")
            };
            
            if virtual_parent_path == "/" {
                format!("/{}", name)
            } else {
                format!("{}/{}", virtual_parent_path, name)
            }
        } else {
            format!("/{}", name)
        };
        

        // Store and get auto-generated inode
        let inode = sync_await(self.drive_item_with_fuse_repo.store_drive_item_with_fuse(&item_with_fuse))?;
        
        // Create ProcessingItem for the local change
        let processing_item = crate::persistency::processing_item_repository::ProcessingItem::new_local(
            drive_item,
            crate::persistency::processing_item_repository::ChangeOperation::Create,
            
        );
        
        let processing_repo = crate::persistency::processing_item_repository::ProcessingItemRepository::new(self.app_state.persistency().pool().clone());
        let _id = sync_await(processing_repo.store_processing_item(&processing_item))?;
        
        debug!("Applied local change: {} {} with inode {} (parent: {:?})", change_type, name, inode, parent_id);
        Ok(inode)
    }

    /// Update item as modified (for write operations)
    async fn mark_db_item_as_modified(&self, ino: u64) -> Result<()> {
        if let Some(mut item) = sync_await(self.get_item_by_ino(ino))? {
            item.set_file_source(FileSource::Local);
            item.set_sync_status("pending".to_string());
            
            // Preserve the existing local_path if it exists
            
            
            // Update the item in database
            // Pass the existing local_path to preserve it
            
            sync_await(self.drive_item_with_fuse_repo.store_drive_item_with_fuse(&item))?;
            
            // Create ProcessingItem for the local modification
            
            
            let processing_repo = crate::persistency::processing_item_repository::ProcessingItemRepository::new(self.app_state.persistency().pool().clone());
            
            // Check if a ProcessingItem already exists for this OneDrive ID
            let onedrive_id = item.id();
            
             

            if let Ok(Some(existing_processing_item)) = sync_await(processing_repo.get_processing_item(onedrive_id)) {
                // Check if the existing ProcessingItem is still in a state where we can update it
                match existing_processing_item.status {
                    crate::persistency::processing_item_repository::ProcessingStatus::New |
                    crate::persistency::processing_item_repository::ProcessingStatus::Validated => {
                        // Update the existing ProcessingItem instead of creating a new one
                        // This "squashes" multiple write operations into a single ProcessingItem
                        debug!("🔄 Updating existing ProcessingItem for OneDrive ID: {} (squashing changes)", onedrive_id);
                        
                        
                        
                        // Update the last_modified timestamp to reflect the latest change
                        let mut updated_drive_item = item.drive_item().clone();
                        updated_drive_item.last_modified = Some(chrono::Utc::now().to_rfc3339());
                        
                        // Update the ProcessingItem with the latest drive item data
                        // Note: We don't update the change_operation since it should remain as Update
                        // The sync processor will handle the actual file content from the local path
                    }
                    _ => {
                        // ProcessingItem is in a different state (processing, done, error, etc.)
                        // Create a new ProcessingItem for this write operation
                        debug!("📝 Creating new ProcessingItem for OneDrive ID: {} (existing item in {:?} state)", 
                               onedrive_id, existing_processing_item.status);
                        
                        let processing_item = crate::persistency::processing_item_repository::ProcessingItem::new_local(
                            item.drive_item().clone(),
                            crate::persistency::processing_item_repository::ChangeOperation::Update
                            
                        );
                        
                        let _id = sync_await(processing_repo.store_processing_item(&processing_item))?;
                    }
                }
            } else {
                // No existing ProcessingItem found, create a new one
                debug!("📝 Creating new ProcessingItem for OneDrive ID: {}", onedrive_id);
                
                let processing_item = crate::persistency::processing_item_repository::ProcessingItem::new_local(
                    item.drive_item().clone(),
                    crate::persistency::processing_item_repository::ChangeOperation::Update,
                    
                );
                
                let _id = sync_await(processing_repo.store_processing_item(&processing_item))?;
            }
        }
        Ok(())
    }
}

impl fuser::Filesystem for OneDriveFuse {
    fn lookup(&mut self, _req: &fuser::Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let name_str = name.to_string_lossy();
        debug!("LOOKUP: parent={}, name={}", parent, name_str);

        // Handle root directory
        if parent == 1 && name_str == "." {
            if let Ok(Some(root_item)) = sync_await(self.get_item_by_ino(1)) {
                reply.entry(
                    &Duration::from_secs(3),
                    &self.item_to_file_attr(&root_item),
                    0,
                );
                return;
            }
        }

        // Strip .onedrivedownload extension if present for lookup
        let lookup_name = if name_str.ends_with(".onedrivedownload") {
            &name_str[..name_str.len() - 17] // Remove ".onedrivedownload"
        } else {
            &name_str
        };

        // Use optimized DB query by parent_ino and name
        if let Ok(Some(item)) = sync_await(self.drive_item_with_fuse_repo.get_drive_item_with_fuse_by_parent_ino_and_name(parent, lookup_name)) {
            reply.entry(
                &Duration::from_secs(3),
                &self.item_to_file_attr(&item),
                0,
            );
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn getattr(&mut self, _req: &fuser::Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        debug!("GETATTR: ino={}", ino);

        if let Ok(Some(item)) = sync_await(self.get_item_by_ino(ino)) {
            reply.attr(
                &Duration::from_secs(3),
                &self.item_to_file_attr(&item),
            );
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn readdir(
        &mut self,
        _req: &fuser::Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        debug!("READDIR: ino={}, offset={}", ino, offset);

        
     


        // We need to get the item anyway to create an entry for ..
        if let Ok(Some(parent_item)) = sync_await(self.get_item_by_ino(ino)) {
            if ino != 1 &&!parent_item.is_folder()  {
                reply.error(libc::ENOTDIR);
                return;
            }
            let onedot_entry = (ino, fuser::FileType::Directory, ".".to_string());
            let twodot_entry = if ino == 1 {
                (1, fuser::FileType::Directory, "..".to_string())
            }else{
                (parent_item.parent_ino().unwrap_or(1), fuser::FileType::Directory, "..".to_string())
            };
            
            
            let mut entries: Vec<(u64, fuser::FileType, String)> = vec![onedot_entry, twodot_entry];
            if let Ok(children) = sync_await(self.get_children_by_parent_ino(ino)) {
                for child in children {
                    let file_type = if child.is_folder() {
                        fuser::FileType::Directory
                    } else {
                        fuser::FileType::RegularFile
                    };
                    let name = if child.is_folder() {
                        child.name().unwrap_or_default().to_string()
                    } else {
                        let base_name = child.name().unwrap_or_default();
                        if self.file_exists_locally(child.id()).is_some() {
                            base_name.to_string()
                        } else {
                            format!("{}.onedrivedownload", base_name)
                        }
                    };
                    entries.push((child.virtual_ino().unwrap_or(0), file_type, name));
                }
            }
            for (i, (ino, kind, name)) in entries.into_iter().enumerate().skip(offset as usize) {
                if reply.add(ino, (i + 1) as i64, kind, &name) {
                    break;
                }
            }
            reply.ok();
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn read(
        &mut self,
        _req: &fuser::Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        debug!("READ: ino={}, offset={}, size={}", ino, offset, size);

        if let Ok(Some(item)) = sync_await(self.get_item_by_ino(ino)) {
            if item.is_folder() {
                reply.error(libc::EISDIR);
                    return;
                }

            // Check if file exists locally with upload folder priority
            if let Some(_) = self.file_exists_locally(item.id()) {
                // File exists locally - read it
                match self.read_local_file(&item, offset as u64, size) {
                    Ok(data) => reply.data(&data),
                    Err(e) => {
                        warn!("Failed to read local file: {}", e);
                        reply.error(libc::EIO);
                    }
                }
                } else {
                // File doesn't exist locally - return placeholder with .onedrivedownload extension
                let placeholder_content = self.generate_placeholder_content(&item);
            let start = offset as usize;
            let end = std::cmp::min(start + size as usize, placeholder_content.len());

            if start < placeholder_content.len() {
                reply.data(&placeholder_content[start..end]);
            } else {
                reply.data(&[]);
                }
            }
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn write(
        &mut self,
        _req: &fuser::Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        info!("WRITE: ino={}, offset={}, data_size={}", ino, offset, data.len());

        if let Ok(Some(item)) = sync_await(self.get_item_by_ino(ino)) {
            let onedrive_id = item.id();
            
            // Always write to upload folder (priority 1)
            let local_path = self.file_manager.get_local_path_if_file_exists(onedrive_id);
            if local_path.is_none() {
                reply.error(libc::ENOENT);
                return;
            }
            let upload_path = local_path.unwrap();
            
            // Write data to uploads file
            let write_result = if offset == 0 {
                std::fs::write(&upload_path, data)
            } else {
                let file_result = std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .open(&upload_path);
                match file_result {
                    Ok(mut file) => {
                        use std::io::Seek;
                        use std::io::SeekFrom;
                        use std::io::Write;
                        if let Err(e) = file.seek(SeekFrom::Start(offset as u64)) {
                            return reply.error(libc::EIO);
                        }
                        file.write_all(data)
                    }
                    Err(e) => Err(e),
                }
            };

            if let Err(e) = write_result {
                error!("Failed to write to uploads: {}", e);
                reply.error(libc::EIO);
                return;
            }

            // Mark item as modified
            if let Err(e) = sync_await(self.mark_db_item_as_modified(ino)) {
                warn!("Failed to mark item as modified: {}", e);
            }

            reply.written(data.len() as u32);
        } else {
                    reply.error(libc::ENOENT);
        }
    }

    fn create(
        &mut self,
        _req: &fuser::Request,
        parent: u64,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        let name_str = name.to_string_lossy();
        info!("CREATE: parent={}, name={}", parent, name_str);

        match sync_await(self.apply_local_change_to_db_repository("create_file", parent, &name_str, false)) {
            Ok(inode) => {
                let attr = self.create_default_attr(inode, false);
                reply.created(&Duration::from_secs(1), &attr, 0, 0, 0);
            }
            Err(e) => {
                error!("Failed to create file: {}", e);
            reply.error(libc::EIO);
            }
        }
    }

    fn mkdir(
        &mut self,
        _req: &fuser::Request,
        parent: u64,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        let name_str = name.to_string_lossy();
        info!("MKDIR: parent={}, name={}", parent, name_str);

        match sync_await(self.apply_local_change_to_db_repository("create_folder", parent, &name_str, true)) {
            Ok(inode) => {
                let attr = self.create_default_attr(inode, true);
                reply.entry(&Duration::from_secs(1), &attr, 0);
            }
            Err(e) => {
                error!("Failed to create directory: {}", e);
            reply.error(libc::EIO);
            }
        }
    }

    fn unlink(
        &mut self,
        _req: &fuser::Request,
        parent: u64,
        name: &OsStr,
        reply: fuser::ReplyEmpty,
    ) {
        let name_str = name.to_string_lossy();
        info!("🗑️ UNLINK: parent={}, name={}", parent, name_str);

        // Find the file to delete by constructing its path
        let file_path = if parent == 1 {
            format!("/{}", name_str)
        } else {
            match sync_await(self.get_item_by_ino(parent)) {
                Ok(Some(parent_item)) => {
                    let parent_path = parent_item.virtual_path().unwrap_or("/");
                    if parent_path == "/" {
                        format!("/{}", name_str)
                    } else {
                        format!("{}/{}", parent_path, name_str)
                    }
                }
                _ => {
                    reply.error(libc::ENOENT);
                    return;
                }
            }
        };

        // Get the file item to delete
        if let Ok(Some(file_item)) = sync_await(self.get_item_by_path(&file_path)) {
            let onedrive_id = file_item.id();
            
            // Create a minimal DriveItem with only ID and deleted field (as per OneDrive API)
            let deleted_drive_item = crate::onedrive_service::onedrive_models::DriveItem {
                id: onedrive_id.to_string(),
                name: None, // Not present in API response for deleted items
                etag: None, // Not present in API response for deleted items
                last_modified: None, // Not present in API response for deleted items
                created_date: None, // Not present in API response for deleted items
                size: None, // Not present in API response for deleted items
                folder: None, // Not present in API response for deleted items
                file: None, // Not present in API response for deleted items
                download_url: None, // Not present in API response for deleted items
                deleted: Some(crate::onedrive_service::onedrive_models::DeletedFacet {
                    state: "deleted".to_string(),
                }),
                parent_reference: None, // Not present in API response for deleted items
            };

            // Create ProcessingItem for the local delete operation
            let processing_item = crate::persistency::processing_item_repository::ProcessingItem::new_local(
                deleted_drive_item,
                crate::persistency::processing_item_repository::ChangeOperation::Delete,
                
            );

            // Store the ProcessingItem in the database
            let processing_repo = crate::persistency::processing_item_repository::ProcessingItemRepository::new(self.app_state.persistency().pool().clone());
            match sync_await(processing_repo.store_processing_item(&processing_item)) {
                Ok(db_id) => {
                    info!("🗑️ Created ProcessingItem for file deletion: {} (DB ID: {})", file_path, db_id);
                    
                    // Mark the file as deleted in the FUSE database
                    let mut updated_file_item = file_item.clone();
                    updated_file_item.drive_item_mut().deleted = Some(crate::onedrive_service::onedrive_models::DeletedFacet {
                        state: "deleted".to_string(),
                    });
                    updated_file_item.set_sync_status("pending".to_string());
                    
                    // Store the updated item (marked as deleted)
                    
                    if let Err(e) = sync_await(self.drive_item_with_fuse_repo.store_drive_item_with_fuse(&updated_file_item)) {
                        error!("Failed to update file as deleted in FUSE database: {}", e);
                        reply.error(libc::EIO);
                        return;
                    }
                    
                    info!("🗑️ Successfully marked file as deleted: {} (inode: {})", file_path, file_item.virtual_ino().unwrap_or(0));
                    reply.ok();
                }
                Err(e) => {
                    error!("Failed to create ProcessingItem for file deletion: {}", e);
                    reply.error(libc::EIO);
                }
            }
        } else {
            error!("File not found: {}", file_path);
            reply.error(libc::ENOENT);
        }
    }

    fn rmdir(
        &mut self,
        _req: &fuser::Request,
        parent: u64,
        name: &OsStr,
        reply: fuser::ReplyEmpty,
    ) {
        let name_str = name.to_string_lossy();
        info!("🗑️ RMDIR: parent={}, name={}", parent, name_str);

        // Find the directory to delete by constructing its path
        let dir_path = if parent == 1 {
            format!("/{}", name_str)
        } else {
            match sync_await(self.get_item_by_ino(parent)) {
                Ok(Some(parent_item)) => {
                    let parent_path = parent_item.virtual_path().unwrap_or("/");
                    if parent_path == "/" {
                        format!("/{}", name_str)
                    } else {
                        format!("{}/{}", parent_path, name_str)
                    }
                }
                _ => {
                    reply.error(libc::ENOENT);
                    return;
                }
            }
        };

        // Get the directory item to delete
        if let Ok(Some(dir_item)) = sync_await(self.get_item_by_path(&dir_path)) {
            let onedrive_id = dir_item.id();
            
            // Create a minimal DriveItem with only ID and deleted field (as per OneDrive API)
            let deleted_drive_item = crate::onedrive_service::onedrive_models::DriveItem {
                id: onedrive_id.to_string(),
                name: None, // Not present in API response for deleted items
                etag: None, // Not present in API response for deleted items
                last_modified: None, // Not present in API response for deleted items
                created_date: None, // Not present in API response for deleted items
                size: None, // Not present in API response for deleted items
                folder: None, // Not present in API response for deleted items
                file: None, // Not present in API response for deleted items
                download_url: None, // Not present in API response for deleted items
                deleted: Some(crate::onedrive_service::onedrive_models::DeletedFacet {
                    state: "deleted".to_string(),
                }),
                parent_reference: None, // Not present in API response for deleted items
            };

            // Create ProcessingItem for the local delete operation
            let processing_item = crate::persistency::processing_item_repository::ProcessingItem::new_local(
                deleted_drive_item,
                crate::persistency::processing_item_repository::ChangeOperation::Delete,
                
            );

            // Store the ProcessingItem in the database
            let processing_repo = crate::persistency::processing_item_repository::ProcessingItemRepository::new(self.app_state.persistency().pool().clone());
            match sync_await(processing_repo.store_processing_item(&processing_item)) {
                Ok(db_id) => {
                    info!("🗑️ Created ProcessingItem for directory deletion: {} (DB ID: {})", dir_path, db_id);
                    
                    // Mark the directory as deleted in the FUSE database
                    let mut updated_dir_item = dir_item.clone();
                    updated_dir_item.drive_item_mut().deleted = Some(crate::onedrive_service::onedrive_models::DeletedFacet {
                        state: "deleted".to_string(),
                    });
                    updated_dir_item.set_sync_status("pending".to_string());
                    
                    // Store the updated item (marked as deleted)
                    
                    if let Err(e) = sync_await(self.drive_item_with_fuse_repo.store_drive_item_with_fuse(&updated_dir_item)) {
                        error!("Failed to update directory as deleted in FUSE database: {}", e);
                        reply.error(libc::EIO);
                        return;
                    }
                    
                    info!("🗑️ Successfully marked directory as deleted: {} (inode: {})", dir_path, dir_item.virtual_ino().unwrap_or(0));
                    reply.ok();
                }
                Err(e) => {
                    error!("Failed to create ProcessingItem for directory deletion: {}", e);
                    reply.error(libc::EIO);
                }
            }
        } else {
            error!("Directory not found: {}", dir_path);
            reply.error(libc::ENOENT);
        }
    }

    fn rename(
        &mut self,
        _req: &fuser::Request,
        parent: u64,
        name: &OsStr,
        newparent: u64,
        newname: &OsStr,
        _flags: u32,
        reply: fuser::ReplyEmpty,
    ) {
        let name_str = name.to_string_lossy();
        let newname_str = newname.to_string_lossy();
        info!("RENAME: parent={}, name={}, newparent={}, newname={}", 
               parent, name_str, newparent, newname_str);

        // Find the item to rename
        let old_path = if parent == 1 {
            format!("/{}", name_str)
        } else {
            match sync_await(self.get_item_by_ino(parent)) {
                Ok(Some(parent_item)) => {
                    let parent_path = parent_item.virtual_path().unwrap_or("/");
                    if parent_path == "/" {
                        format!("/{}", name_str)
                    } else {
                        format!("{}/{}", parent_path, name_str)
                    }
                }
                _ => {
                    reply.error(libc::ENOENT);
                    return;
                }
            }
        };

        // Get the item to rename
        if let Ok(Some(mut item)) = sync_await(self.get_item_by_path(&old_path)) {
            // Update the item with new name and parent
            item.drive_item_mut().name = Some(newname_str.to_string());
            
            // Update parent reference if parent changed
            if parent != newparent {
                if let Ok(Some(new_parent_item)) = sync_await(self.get_item_by_ino(newparent)) {
                    item.set_parent_ino(newparent);
                    item.drive_item_mut().parent_reference = Some(crate::onedrive_service::onedrive_models::ParentReference {
                        id: new_parent_item.id().to_string(),
                        path: new_parent_item.virtual_path().map(|p| p.to_string()),
                    });
                }
            }

            // Update virtual path and display path
            let new_virtual_path = if newparent == 1 {
                format!("/{}", newname_str)
        } else {
                match sync_await(self.get_item_by_ino(newparent)) {
                    Ok(Some(new_parent_item)) => {
                        let parent_path = new_parent_item.virtual_path().unwrap_or("/");
                        if parent_path == "/" {
                            format!("/{}", newname_str)
                        } else {
                            format!("{}/{}", parent_path, newname_str)
                        }
                    }
                    _ => {
                        reply.error(libc::ENOENT);
                        return;
                    }
                }
            };
            
            item.set_virtual_path(new_virtual_path.clone());
            
            
            // Mark as modified
            item.set_file_source(FileSource::Local);
            item.set_sync_status("pending".to_string());
            // Update parent ino
            item.set_parent_ino(newparent);
            // Update paretn id ( onedriveId of parent)
            let new_parent_item = sync_await(self.get_item_by_ino(newparent)).unwrap_or(None);
            if let Some(new_parent_item) = new_parent_item {
                item.set_parent_id(new_parent_item.drive_item.id);
            }
            

            // Preserve the existing local_path if it exists
            
            
            // Store the updated item (this will now use UPDATE instead of INSERT OR REPLACE)
            // Pass the existing local_path to preserve it
            
            match sync_await(self.drive_item_with_fuse_repo.store_drive_item_with_fuse(&item)) {
                Ok(_) => {
                    info!("Successfully renamed {} to {} (inode: {})", 
                          name_str, newname_str, item.virtual_ino().unwrap_or(0));
                    
                    // Create ProcessingItem for the rename/move operation
                    let old_name = name_str.to_string();
                    let new_name = newname_str.to_string();
                    let is_move = parent != newparent;
                    
                    let change_operation = if is_move {
                        crate::persistency::processing_item_repository::ChangeOperation::Move {
                            old_path: old_path.clone(),
                            new_path: new_virtual_path.clone(),
                        }
                    } else {
                        crate::persistency::processing_item_repository::ChangeOperation::Rename {
                            old_name,
                            new_name,
                        }
                    };
                    
                    let processing_item = crate::persistency::processing_item_repository::ProcessingItem::new_local(
                        item.drive_item().clone(),
                        change_operation,
                        
                    );
                    
                    // Store the ProcessingItem in the database
                    let processing_repo = crate::persistency::processing_item_repository::ProcessingItemRepository::new(self.app_state.persistency().pool().clone());
                    match sync_await(processing_repo.store_processing_item(&processing_item)) {
                        Ok(db_id) => {
                            let operation_type = if is_move { "move" } else { "rename" };
                            info!("📁 Created ProcessingItem for {}: {} -> {} (DB ID: {})", 
                                  operation_type, old_path, new_virtual_path, db_id);
                        }
                        Err(e) => {
                            warn!("Failed to create ProcessingItem for rename/move: {}", e);
                            // Don't fail the operation, just log the warning
                        }
                    }
                    
                    reply.ok();
                }
                Err(e) => {
                    error!("Failed to rename {} to {}: {}", name_str, newname_str, e);
                    reply.error(libc::EIO);
                }
            }
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn setattr(
        &mut self,
        _req: &fuser::Request,
        ino: u64,
        _file_handle: Option<u32>,
        _to_set: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u64>,
        size: Option<fuser::TimeOrNow>,
        _atime: Option<fuser::TimeOrNow>,
        mtime: Option<SystemTime>,
        _ctime: Option<u64>,
        _fh: Option<SystemTime>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<u32>,
        reply: ReplyAttr,
    ) {
        debug!("SETATTR: ino={}", ino);

        if let Ok(Some(item)) = sync_await(self.get_item_by_ino(ino)) {
            // Mark as modified if any attributes changed
            if size.is_some() || mtime.is_some() {
                if let Err(e) = sync_await(self.mark_db_item_as_modified(ino)) {
                    warn!("Failed to mark item as modified: {}", e);
                }
            }

            reply.attr(&Duration::from_secs(1), &self.item_to_file_attr(&item));
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn statfs(&mut self, _req: &fuser::Request, _ino: u64, reply: ReplyStatfs) {
        debug!("STATFS");

        // Return dummy filesystem statistics
        reply.statfs(
            1_000_000_000, // Total blocks
            500_000_000,   // Free blocks
            500_000_000,   // Available blocks
            1_000_000,     // Total files
            500_000,       // Free files
            512,           // Block size
            255,           // Max filename length
            0,             // Fragment size
        );
    }
    fn init(
        &mut self,
        _req: &fuser::Request<'_>,
        config: &mut KernelConfig,
    ) -> Result<(), c_int> {
        config.add_capabilities(FUSE_CAP_READDIRPLUS).expect("Failed to add capabilities");
        Ok(())
    }

    fn readdirplus(
        &mut self,
        _req: &fuser::Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: fuser::ReplyDirectoryPlus,
    ) {
        warn!("READDIRPLUS: ino={}, offset={}", ino, offset);

  

        // Handle regular directories
        if let Ok(Some(parent_item)) = sync_await(self.get_item_by_ino(ino)) {
            if !parent_item.is_folder() {
                reply.error(libc::ENOTDIR);
                return;
            }
            let onedot_entry = (ino, fuser::FileType::Directory, ".".to_string());
            let twodot_entry = if ino == 1 {
                (1, fuser::FileType::Directory, "..".to_string())
            }else{
                (parent_item.parent_ino().unwrap_or(1), fuser::FileType::Directory, "..".to_string())
            };
            let mut entries: Vec<(u64, fuser::FileType, String)> = vec![onedot_entry, twodot_entry];
            if let Ok(children) = sync_await(self.get_children_by_parent_ino(ino)) {
                for child in children {
                    let file_type = if child.is_folder() {
                        fuser::FileType::Directory
                    } else {
                        fuser::FileType::RegularFile
                    };
                    let name = if child.is_folder() {
                        child.name().unwrap_or_default().to_string()
                    } else {
                        let base_name = child.name().unwrap_or_default();
                        if self.file_exists_locally(child.id()).is_some() {
                            base_name.to_string()
                        } else {
                            format!("{}.onedrivedownload", base_name)
                        }
                    };
                    entries.push((child.virtual_ino().unwrap_or(0), file_type, name));
                }
            }
            for (i, (ino, kind, name)) in entries.iter().enumerate().skip(offset as usize) {
                let attr = if let Ok(Some(item)) = sync_await(self.get_item_by_ino(*ino)) {
                    self.item_to_file_attr(&item)
                } else {
                    self.create_default_attr(*ino, kind == &fuser::FileType::Directory)
                };
                if reply.add(*ino, (i + 1) as i64, &name, &Duration::from_secs(3), &attr, 0) {
                    break;
                }
            }
            reply.ok();
        } else {
            reply.error(libc::ENOENT);
        }
    }
}
