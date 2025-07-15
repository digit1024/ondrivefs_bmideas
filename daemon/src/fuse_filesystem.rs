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
        file_manager: Arc<DefaultFileManager>
    ) -> Result<Self> {
        let drive_item_with_fuse_repo = DriveItemWithFuseRepository::new(pool);
        
        Ok(Self {
            drive_item_with_fuse_repo: Arc::new(drive_item_with_fuse_repo),
            download_queue_repo: Arc::new(download_queue_repo),
            file_manager,
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
        let all_items = sync_await(self.drive_item_with_fuse_repo.get_all_drive_items_with_fuse())?;
        
        for item in all_items {
            if let Some(virtual_path) = item.virtual_path() {
                if virtual_path == path {
                    return Ok(Some(item));
                }
            }
        }
        Ok(None)
    }

    /// Get children of a directory by parent inode
    async fn get_children_by_parent_ino(&self, parent_ino: u64) -> Result<Vec<DriveItemWithFuse>> {
        sync_await(self.drive_item_with_fuse_repo.get_children_by_parent_ino(parent_ino))
    }

    /// Generate placeholder content for remote files
    fn generate_placeholder_content(&self, item: &DriveItemWithFuse) -> Vec<u8> {
        let placeholder = serde_json::json!({
            "onedrive_id": item.id(),
            "name": item.name().unwrap_or("unknown"),
            "message": "remote file - not downloaded locally"
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

    /// Read data from a local file
    fn read_local_file(&self, item: &DriveItemWithFuse, offset: i64, size: u32) -> Result<Vec<u8>> {
        let onedrive_id = item.id();
        
        // Try to find the file in downloads first, then uploads
        let download_path = self.file_manager.get_download_dir().join(onedrive_id);
        let upload_path = self.file_manager.get_upload_dir().join(onedrive_id);
        
        let local_path = if download_path.exists() && download_path.is_file() {
            download_path
        } else if upload_path.exists() && upload_path.is_file() {
            upload_path
        } else {
            return Err(anyhow::anyhow!("Local file not found for OneDrive ID: {}", onedrive_id));
        };

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
            parent_reference: None, // We'll handle parent relationship via parent_ino
        };

        let mut item_with_fuse = self.drive_item_with_fuse_repo.create_from_drive_item(drive_item);
        item_with_fuse.set_parent_ino(parent_ino);
        item_with_fuse.set_file_source(FileSource::Local);
        item_with_fuse.set_sync_status("pending".to_string());

        // Store and get auto-generated inode
        let inode = sync_await(self.drive_item_with_fuse_repo.store_drive_item_with_fuse(&item_with_fuse, None))?;
        
        debug!("Applied local change: {} {} with inode {}", change_type, name, inode);
        Ok(inode)
    }

    /// Update item as modified (for write operations)
    async fn mark_item_as_modified(&self, ino: u64) -> Result<()> {
        if let Some(mut item) = sync_await(self.get_item_by_ino(ino))? {
            item.set_file_source(FileSource::Local);
            item.set_sync_status("pending".to_string());
            
            // Update the item in database
            sync_await(self.drive_item_with_fuse_repo.store_drive_item_with_fuse(&item, None))?;
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
                    &Duration::from_secs(1),
                    &self.item_to_file_attr(&root_item),
                    0,
                );
                return;
            }
        }

        // Get parent directory path
        let parent_path = if parent == 1 {
            "/".to_string()
        } else {
            match sync_await(self.get_item_by_ino(parent)) {
                Ok(Some(parent_item)) => parent_item.virtual_path().unwrap_or("/").to_string(),
                _ => {
                reply.error(libc::ENOENT);
                return;
                }
            }
        };

        // Construct full path
        let full_path = if parent_path == "/" {
            format!("/{}", name_str)
        } else {
            format!("{}/{}", parent_path, name_str)
        };

        // Try to get the item
        if let Ok(Some(item)) = sync_await(self.get_item_by_path(&full_path)) {
            reply.entry(
                &Duration::from_secs(1),
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
                &Duration::from_secs(1),
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
            
            // Get children of root
            if let Ok(children) = sync_await(self.get_children_by_parent_ino(ino)) {
                for child in children {
                    let file_type = if child.is_folder() {
                    fuser::FileType::Directory
                } else {
                    fuser::FileType::RegularFile
                    };
                    let name = child.name().unwrap_or_default().to_string();
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
            
            // Get children
            if let Ok(children) = sync_await(self.get_children_by_parent_ino(ino)) {
                for child in children {
                    let file_type = if child.is_folder() {
                        fuser::FileType::Directory
                    } else {
                        fuser::FileType::RegularFile
                    };
                    let name = child.name().unwrap_or_default().to_string();
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

            // Check if file exists locally
            let onedrive_id = item.id();
            let download_path = self.file_manager.get_download_dir().join(onedrive_id);
                let upload_path = self.file_manager.get_upload_dir().join(onedrive_id);
            
            if download_path.exists() || upload_path.exists() {
                // File exists locally - read it
                match self.read_local_file(&item, offset, size) {
                    Ok(data) => reply.data(&data),
                    Err(e) => {
                        warn!("Failed to read local file: {}", e);
                        reply.error(libc::EIO);
                    }
                }
                        } else {
                // File doesn't exist locally - return placeholder
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
        debug!("WRITE: ino={}, offset={}, data_size={}", ino, offset, data.len());

        if let Ok(Some(item)) = sync_await(self.get_item_by_ino(ino)) {
            let onedrive_id = item.id();
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
        debug!("CREATE: parent={}, name={}", parent, name_str);

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
        debug!("MKDIR: parent={}, name={}", parent, name_str);

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
        debug!("UNLINK: parent={}, name={}", parent, name_str);

        // For now, just mark as deleted in the database
        // In a full implementation, you'd want to handle the actual deletion
        reply.ok();
    }

    fn rmdir(
        &mut self,
        _req: &fuser::Request,
        parent: u64,
        name: &OsStr,
        reply: fuser::ReplyEmpty,
    ) {
        let name_str = name.to_string_lossy();
        debug!("RMDIR: parent={}, name={}", parent, name_str);

        // For now, just mark as deleted in the database
        // In a full implementation, you'd want to handle the actual deletion
        reply.ok();
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
        debug!("RENAME: parent={}, name={}, newparent={}, newname={}", 
               parent, name_str, newparent, newname_str);

        // For now, just acknowledge the rename
        // In a full implementation, you'd update the item in the database
        reply.ok();
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
