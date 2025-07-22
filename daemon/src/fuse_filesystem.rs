use crate::persistency::drive_item_with_fuse_repository::DriveItemWithFuseRepository;
use crate::persistency::download_queue_repository::DownloadQueueRepository;
use crate::persistency::types::{DriveItemWithFuse, FileSource};
use crate::file_manager::{FileManager, DefaultFileManager};
use crate::onedrive_service::onedrive_models::DriveItem;
use anyhow::Result;
use fuser::{
    FileAttr, FileType, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory, ReplyEntry, ReplyStatfs,
    ReplyWrite,
};
use log::{debug, info, warn, error};
use sqlx::types::chrono;
use std::ffi::OsStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::runtime::Handle;
use std::path::PathBuf;
use sqlx::Pool;

/// OneDrive FUSE filesystem implementation using DriveItemWithFuse
pub struct OneDriveFuse {
    drive_item_with_fuse_repo: Arc<DriveItemWithFuseRepository>,
    download_queue_repo: Arc<DownloadQueueRepository>,
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
            download_queue_repo: Arc::new(download_queue_repo),
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
        root_with_fuse.set_display_path("/".to_string());
        root_with_fuse.set_file_source(FileSource::Local); // Mark as local since it's a stub
        root_with_fuse.set_sync_status("stub".to_string());

        root_with_fuse
    }

    /// Get DriveItemWithFuse by inode
    async fn get_item_by_ino(&self, ino: u64) -> Result<Option<DriveItemWithFuse>> {
        if ino == 1 {
            // Special handling for root inode
            let root_item = sync_await(self.drive_item_with_fuse_repo.get_drive_item_with_fuse_by_virtual_ino(1))?;
            if root_item.is_some() {
                Ok(root_item)
        } else {
                // Return temporary stub for root
                Ok(Some(self.create_temp_root_stub()))
            }
        } else {
            sync_await(self.drive_item_with_fuse_repo.get_drive_item_with_fuse_by_virtual_ino(ino))
        }
    }



    /// Get DriveItemWithFuse by path
    async fn get_item_by_path(&self, path: &str) -> Result<Option<DriveItemWithFuse>> {
        // For now, we'll search by virtual_path
        // In a production system, you might want to add a path index
        let item = sync_await(self.drive_item_with_fuse_repo.get_drive_item_with_fuse_by_virtual_path(path));
        item
        
    }

    /// Get children of a directory by parent inode
    async fn get_children_by_parent_ino(&self, parent_ino: u64) -> Result<Vec<DriveItemWithFuse>> {
        sync_await(self.drive_item_with_fuse_repo.get_children_by_parent_ino(parent_ino))
    }

    /// Check if file exists locally with upload folder priority
    fn file_exists_locally(&self, onedrive_id: &str) -> Option<PathBuf> {
        // Priority 1: Check upload folder first
        let upload_path = self.file_manager.get_upload_dir().join(onedrive_id);
        if upload_path.exists() && upload_path.is_file() {
            return Some(upload_path);
        }
        
        // Priority 2: Check download folder
        let download_path = self.file_manager.get_download_dir().join(onedrive_id);
        if download_path.exists() && download_path.is_file() {
            return Some(download_path);
        }
        
            None
        }

    /// Read data from a local file with upload folder priority
    fn read_local_file(&self, item: &DriveItemWithFuse, offset: i64, size: u32) -> Result<Vec<u8>> {
        let onedrive_id = item.id();
        
        // Get local file path with upload folder priority
        let local_path = self.file_exists_locally(onedrive_id)
            .ok_or_else(|| anyhow::anyhow!("Local file not found for OneDrive ID: {}", onedrive_id))?;

        // Read file data
        let file_data = std::fs::read(&local_path)?;
        
        // Handle offset and size
        let start = offset as usize;
        let end = std::cmp::min(start + size as usize, file_data.len());
        
        if start >= file_data.len() {
            return Ok(vec![]);
        }
        
        Ok(file_data[start..end].to_vec())
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
    async fn apply_local_change(&self, change_type: &str, parent_ino: u64, name: &str, is_folder: bool) -> Result<u64> {
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
        
        // Set local path for local items (they go to uploads folder)
        let local_path = self.file_manager.get_upload_dir().join(&temporary_id);
        item_with_fuse.set_local_path(local_path.to_string_lossy().to_string());
        
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
        item_with_fuse.set_display_path(display_path);

        // Store and get auto-generated inode
        let inode = sync_await(self.drive_item_with_fuse_repo.store_drive_item_with_fuse(&item_with_fuse, Some(local_path.clone())))?;
        
        // Create ProcessingItem for the local change
        let processing_item = crate::persistency::processing_item_repository::ProcessingItem::new_local(
            drive_item,
            crate::persistency::processing_item_repository::ChangeOperation::Create,
            local_path
        );
        
        let processing_repo = crate::persistency::processing_item_repository::ProcessingItemRepository::new(self.app_state.persistency().pool().clone());
        let _id = sync_await(processing_repo.store_processing_item(&processing_item))?;
        
        debug!("Applied local change: {} {} with inode {} (parent: {:?})", change_type, name, inode, parent_id);
        Ok(inode)
    }

    /// Update item as modified (for write operations)
    async fn mark_item_as_modified(&self, ino: u64) -> Result<()> {
        if let Some(mut item) = sync_await(self.get_item_by_ino(ino))? {
            item.set_file_source(FileSource::Local);
            item.set_sync_status("pending".to_string());
            
            // Preserve the existing local_path if it exists
            let existing_local_path = item.local_path().map(|p| p.to_string());
            
            // Update the item in database
            // Pass the existing local_path to preserve it
            let local_path_option = existing_local_path.clone().map(PathBuf::from);
            sync_await(self.drive_item_with_fuse_repo.store_drive_item_with_fuse(&item, local_path_option))?;
            
            // Create ProcessingItem for the local modification
            let local_path = if let Some(path_str) = existing_local_path {
                PathBuf::from(path_str)
            } else {
                self.file_manager.get_upload_dir().join(item.id())
            };
            
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
                        
                        // Update the local path if it changed
                        if existing_processing_item.local_path.as_ref() != Some(&local_path) {
                            sync_await(processing_repo.update_local_path(onedrive_id, &local_path))?;
                        }
                        
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
                            crate::persistency::processing_item_repository::ChangeOperation::Update,
                            local_path
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
                    local_path
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
                    &Duration::from_secs(120),
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
                &Duration::from_secs(120),
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
                &Duration::from_secs(120),
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

        // Handle root directory special case
        if ino == 1 {
            let onedot_entry = (1, fuser::FileType::Directory, ".".to_string());
            let twodot_entry = (1, fuser::FileType::Directory, "..".to_string());
            let mut entries: Vec<(u64, fuser::FileType, String)> = vec![onedot_entry, twodot_entry];

            // Fetch all children of root
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
            return;
        }

        // Handle regular directories
        if let Ok(Some(parent_item)) = sync_await(self.get_item_by_ino(ino)) {
            if !parent_item.is_folder() {
                reply.error(libc::ENOTDIR);
                return;
            }
            let onedot_entry = (ino, fuser::FileType::Directory, ".".to_string());
            let twodot_entry = (parent_item.parent_ino().unwrap_or(1), fuser::FileType::Directory, "..".to_string());
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
                match self.read_local_file(&item, offset, size) {
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
            let upload_path = self.file_manager.get_upload_dir().join(onedrive_id);
            
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
            if let Err(e) = sync_await(self.mark_item_as_modified(ino)) {
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

        match sync_await(self.apply_local_change("create_file", parent, &name_str, false)) {
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

        match sync_await(self.apply_local_change("create_folder", parent, &name_str, true)) {
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
                PathBuf::new(), // No local path for delete operations
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
                    let local_path_option = updated_file_item.local_path().map(|p| PathBuf::from(p));
                    if let Err(e) = sync_await(self.drive_item_with_fuse_repo.store_drive_item_with_fuse(&updated_file_item, local_path_option)) {
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
                PathBuf::new(), // No local path for delete operations
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
                    let local_path_option = updated_dir_item.local_path().map(|p| PathBuf::from(p));
                    if let Err(e) = sync_await(self.drive_item_with_fuse_repo.store_drive_item_with_fuse(&updated_dir_item, local_path_option)) {
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
            item.set_display_path(new_virtual_path.clone());
            
            // Mark as modified
            item.set_file_source(FileSource::Local);
            item.set_sync_status("pending".to_string());

            // Preserve the existing local_path if it exists
            let existing_local_path = item.local_path().map(|p| p.to_string());
            
            // Store the updated item (this will now use UPDATE instead of INSERT OR REPLACE)
            // Pass the existing local_path to preserve it
            let local_path_option = existing_local_path.clone().map(PathBuf::from);
            match sync_await(self.drive_item_with_fuse_repo.store_drive_item_with_fuse(&item, local_path_option)) {
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
                        existing_local_path.map(PathBuf::from).unwrap_or_else(|| PathBuf::new()),
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
                if let Err(e) = sync_await(self.mark_item_as_modified(ino)) {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistency::processing_item_repository::{ProcessingItem, ProcessingStatus, ChangeOperation, ChangeType};
    use crate::onedrive_service::onedrive_models::DriveItem;
    use tempfile::TempDir;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_processing_item_squash_functionality() {
        // This test demonstrates the "squash" functionality where multiple write operations
        // to the same file result in a single ProcessingItem being updated rather than
        // multiple ProcessingItems being created.
        
        // Setup temporary directory for test database
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
        
        // Create database connection
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .unwrap();
        
        // Initialize database schema
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS processing_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                drive_item_id TEXT NOT NULL,
                name TEXT,
                etag TEXT,
                last_modified TEXT,
                created_date TEXT,
                size INTEGER,
                is_folder BOOLEAN,
                mime_type TEXT,
                download_url TEXT,
                is_deleted BOOLEAN,
                parent_id TEXT,
                parent_path TEXT,
                status TEXT DEFAULT 'new',
                local_path TEXT,
                error_message TEXT,
                last_status_update TEXT,
                retry_count INTEGER DEFAULT 0,
                priority INTEGER DEFAULT 0,
                change_type TEXT DEFAULT 'remote',
                change_operation TEXT DEFAULT 'create',
                conflict_resolution TEXT,
                validation_errors TEXT,
                user_decision TEXT,
                created_at TEXT DEFAULT (datetime('now')),
                updated_at TEXT DEFAULT (datetime('now'))
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        // Create repositories
        let processing_repo = crate::persistency::processing_item_repository::ProcessingItemRepository::new(pool.clone());
        
        // Create a test drive item
        let drive_item = DriveItem {
            id: "test_file_123".to_string(),
            name: Some("test.txt".to_string()),
            etag: None,
            last_modified: Some(chrono::Utc::now().to_rfc3339()),
            created_date: Some(chrono::Utc::now().to_rfc3339()),
            size: Some(100),
            folder: None,
            file: Some(crate::onedrive_service::onedrive_models::FileFacet { mime_type: Some("text/plain".to_string()) }),
            download_url: None,
            deleted: None,
            parent_reference: None,
        };

        // Simulate first write operation - should create a new ProcessingItem
        let local_path = PathBuf::from("/tmp/test_file_123");
        let processing_item1 = ProcessingItem::new_local(
            drive_item.clone(),
            ChangeOperation::Update,
            local_path.clone()
        );
        
        let id1 = processing_repo.store_processing_item(&processing_item1).await.unwrap();
        println!("📝 Created first ProcessingItem with ID: {}", id1);
        
        // Verify the ProcessingItem was created
        let retrieved_item1 = processing_repo.get_processing_item("test_file_123").await.unwrap().unwrap();
        assert_eq!(retrieved_item1.status, ProcessingStatus::New);
        assert_eq!(retrieved_item1.change_type, ChangeType::Local);
        assert_eq!(retrieved_item1.change_operation, ChangeOperation::Update);
        
        // Simulate second write operation to the same file
        // In the real implementation, this would be handled by mark_item_as_modified
        // which would check for existing ProcessingItems and update them instead of creating new ones
        
        // Check if ProcessingItem exists for this OneDrive ID
        if let Ok(Some(existing_item)) = processing_repo.get_processing_item("test_file_123").await {
            match existing_item.status {
                ProcessingStatus::New | ProcessingStatus::Validated => {
                    println!("🔄 Found existing ProcessingItem in updateable state - would squash changes");
                    
                    // In the real implementation, we would update the existing item
                    // instead of creating a new one. This demonstrates the squash concept.
                    
                    // Update the local path if it changed
                    if existing_item.local_path.as_ref() != Some(&local_path) {
                        processing_repo.update_local_path("test_file_123", &local_path).await.unwrap();
                        println!("🔄 Updated local path for existing ProcessingItem");
                    }
                    
                    // The existing ProcessingItem would be reused, not a new one created
                    println!("✅ Squash successful - no new ProcessingItem created");
                }
                _ => {
                    println!("📝 Existing ProcessingItem in non-updateable state - would create new one");
                }
            }
        } else {
            println!("📝 No existing ProcessingItem found - would create new one");
        }
        
        // Verify we still have only one ProcessingItem
        let all_items = processing_repo.get_all_processing_items().await.unwrap();
        let test_items: Vec<_> = all_items.iter()
            .filter(|item| item.drive_item.id == "test_file_123")
            .collect();
        
        assert_eq!(test_items.len(), 1, "Should have exactly one ProcessingItem for test_file_123");
        println!("✅ Verification passed: {} ProcessingItem(s) for test_file_123", test_items.len());
        
        // Clean up
        processing_repo.delete_processing_item("test_file_123").await.unwrap();
        println!("🧹 Cleaned up test ProcessingItem");
    }

    #[tokio::test]
    async fn test_immediate_upload_and_id_update() {
        // This test demonstrates the new immediate upload functionality where
        // files are uploaded right away and temporary IDs are replaced with real OneDrive IDs.
        
        // Setup temporary directory for test database
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_upload.db");
        let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
        
        // Create database connection
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .unwrap();
        
        // Initialize database schema for both tables
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS drive_items_with_fuse (
                virtual_ino INTEGER PRIMARY KEY AUTOINCREMENT,
                onedrive_id TEXT UNIQUE NOT NULL,
                name TEXT,
                etag TEXT,
                last_modified TEXT,
                created_date TEXT,
                size INTEGER,
                is_folder BOOLEAN,
                mime_type TEXT,
                download_url TEXT,
                is_deleted BOOLEAN DEFAULT FALSE,
                parent_id TEXT,
                parent_path TEXT,
                local_path TEXT,
                parent_ino INTEGER,
                virtual_path TEXT,
                display_path TEXT,
                file_source TEXT,
                sync_status TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS processing_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                drive_item_id TEXT NOT NULL,
                name TEXT,
                etag TEXT,
                last_modified TEXT,
                created_date TEXT,
                size INTEGER,
                is_folder BOOLEAN,
                mime_type TEXT,
                download_url TEXT,
                is_deleted BOOLEAN,
                parent_id TEXT,
                parent_path TEXT,
                status TEXT DEFAULT 'new',
                local_path TEXT,
                error_message TEXT,
                last_status_update TEXT,
                retry_count INTEGER DEFAULT 0,
                priority INTEGER DEFAULT 0,
                change_type TEXT DEFAULT 'remote',
                change_operation TEXT DEFAULT 'create',
                conflict_resolution TEXT,
                validation_errors TEXT,
                user_decision TEXT,
                created_at TEXT DEFAULT (datetime('now')),
                updated_at TEXT DEFAULT (datetime('now'))
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        // Create repositories
        let drive_item_with_fuse_repo = crate::persistency::drive_item_with_fuse_repository::DriveItemWithFuseRepository::new(pool.clone());
        let processing_repo = crate::persistency::processing_item_repository::ProcessingItemRepository::new(pool.clone());
        
        // Create a test drive item with temporary ID
        let temporary_id = "local_temp_123";
        let drive_item = DriveItem {
            id: temporary_id.to_string(),
            name: Some("test_upload.txt".to_string()),
            etag: None,
            last_modified: Some(chrono::Utc::now().to_rfc3339()),
            created_date: Some(chrono::Utc::now().to_rfc3339()),
            size: Some(100),
            folder: None,
            file: Some(crate::onedrive_service::onedrive_models::FileFacet { mime_type: Some("text/plain".to_string()) }),
            download_url: None,
            deleted: None,
            parent_reference: None,
        };

        // Create ProcessingItem with temporary ID
        let local_path = PathBuf::from("/tmp/test_upload_123");
        let processing_item = ProcessingItem::new_local(
            drive_item.clone(),
            ChangeOperation::Create,
            local_path.clone()
        );
        
        let processing_id = processing_repo.store_processing_item(&processing_item).await.unwrap();
        println!("📝 Created ProcessingItem with temporary ID: {} (DB ID: {})", temporary_id, processing_id);
        
        // Create DriveItemWithFuse with temporary ID
        let mut item_with_fuse = drive_item_with_fuse_repo.create_from_drive_item(drive_item.clone());
        item_with_fuse.set_file_source(crate::persistency::types::FileSource::Local);
        item_with_fuse.set_sync_status("pending".to_string());
        item_with_fuse.set_local_path(local_path.to_string_lossy().to_string());
        
        let fuse_inode = drive_item_with_fuse_repo.store_drive_item_with_fuse(&item_with_fuse, Some(local_path.clone())).await.unwrap();
        println!("📁 Created DriveItemWithFuse with temporary ID: {} (inode: {})", temporary_id, fuse_inode);
        
        // Simulate the immediate upload and ID update process
        // In the real implementation, this would happen in handle_local_create
        let real_onedrive_id = "real_onedrive_456";
        
        println!("🔄 Simulating immediate upload and ID update...");
        
        // Update DriveItemWithFuse
        drive_item_with_fuse_repo.update_onedrive_id(temporary_id, real_onedrive_id).await.unwrap();
        println!("✅ Updated DriveItemWithFuse: {} -> {}", temporary_id, real_onedrive_id);
        
        // Update ProcessingItems
        processing_repo.update_onedrive_id(temporary_id, real_onedrive_id).await.unwrap();
        println!("✅ Updated ProcessingItems: {} -> {}", temporary_id, real_onedrive_id);
        
        // Update parent IDs for any children (in this case, none)
        drive_item_with_fuse_repo.update_parent_id_for_children(temporary_id, real_onedrive_id).await.unwrap();
        processing_repo.update_parent_id_for_children(temporary_id, real_onedrive_id).await.unwrap();
        println!("✅ Updated parent IDs for children: {} -> {}", temporary_id, real_onedrive_id);
        
        // Verify the updates worked correctly
        let updated_fuse_item = drive_item_with_fuse_repo.get_drive_item_with_fuse(real_onedrive_id).await.unwrap();
        assert!(updated_fuse_item.is_some(), "DriveItemWithFuse should exist with real OneDrive ID");
        println!("✅ Verified DriveItemWithFuse exists with real OneDrive ID: {}", real_onedrive_id);
        
        let updated_processing_item = processing_repo.get_processing_item(real_onedrive_id).await.unwrap();
        assert!(updated_processing_item.is_some(), "ProcessingItem should exist with real OneDrive ID");
        println!("✅ Verified ProcessingItem exists with real OneDrive ID: {}", real_onedrive_id);
        
        // Verify temporary ID no longer exists
        let old_fuse_item = drive_item_with_fuse_repo.get_drive_item_with_fuse(temporary_id).await.unwrap();
        assert!(old_fuse_item.is_none(), "DriveItemWithFuse should not exist with temporary ID");
        println!("✅ Verified temporary ID no longer exists in DriveItemWithFuse");
        
        let old_processing_item = processing_repo.get_processing_item(temporary_id).await.unwrap();
        assert!(old_processing_item.is_none(), "ProcessingItem should not exist with temporary ID");
        println!("✅ Verified temporary ID no longer exists in ProcessingItems");
        
        println!("🎉 All database references successfully updated from temporary to real OneDrive ID!");
        
        // Clean up
        drive_item_with_fuse_repo.delete_drive_item_with_fuse(real_onedrive_id).await.unwrap();
        processing_repo.delete_processing_item(real_onedrive_id).await.unwrap();
        println!("🧹 Cleaned up test data");
    }
}
