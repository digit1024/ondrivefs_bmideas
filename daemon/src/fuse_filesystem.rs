use crate::persistency::fuse_repository::FuseRepository;
use crate::persistency::download_queue_repository::DownloadQueueRepository;
use crate::persistency::types::{VirtualFile, FileSource};
use crate::file_manager::{FileManager, DefaultFileManager};
use anyhow::Result;
use fuser::{
    FileAttr, FileType, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory, ReplyEntry, ReplyStatfs,
    ReplyWrite,
};
use log::{debug, info, warn};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::runtime::Handle;
use tokio::sync::RwLock;
use std::path::PathBuf;
use sqlx::Pool;

/// OneDrive FUSE filesystem implementation
pub struct OneDriveFuse {
    fuse_repo: Arc<Mutex<FuseRepository>>,
    download_queue_repo: Arc<DownloadQueueRepository>,
    file_manager: Arc<DefaultFileManager>,
    inode_map: Arc<Mutex<HashMap<u64, VirtualFile>>>,
    path_map: Arc<Mutex<HashMap<String, u64>>>,
}
fn sync_await<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    tokio::task::block_in_place(|| Handle::current().block_on(future))
}

impl OneDriveFuse {
    /// Create a new OneDrive FUSE filesystem
    pub async fn new(fuse_repo: FuseRepository, download_queue_repo: DownloadQueueRepository, file_manager: Arc<DefaultFileManager>) -> Result<Self> {
        Ok(Self {
            fuse_repo: Arc::new(Mutex::new(fuse_repo)),
            download_queue_repo: Arc::new(download_queue_repo),
            file_manager,
            inode_map: Arc::new(Mutex::new(HashMap::new())),
            path_map: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Create a new OneDrive FUSE filesystem with file manager integration
    pub async fn new_with_file_manager(
        pool: Pool<sqlx::Sqlite>, 
        download_queue_repo: DownloadQueueRepository, 
        file_manager: Arc<DefaultFileManager>
    ) -> Result<Self> {
        // Create FuseRepository with file manager for local file checking
        let fuse_repo = FuseRepository::new_with_file_manager(
            pool, 
            Box::new(file_manager.as_ref().clone())
        );
        
        Ok(Self {
            fuse_repo: Arc::new(Mutex::new(fuse_repo)),
            download_queue_repo: Arc::new(download_queue_repo),
            file_manager,
            inode_map: Arc::new(Mutex::new(HashMap::new())),
            path_map: Arc::new(Mutex::new(HashMap::new())),
        })
    }

        /// Add a VirtualFile to the inode and path maps
        fn cache_virtual_file(&self, virtual_file: &VirtualFile) {
            let mut inode_map = self.inode_map.lock().unwrap();
            let mut path_map = self.path_map.lock().unwrap();
            inode_map.insert(virtual_file.ino, virtual_file.clone());
            path_map.insert(virtual_file.virtual_path.clone(), virtual_file.ino);
            // Also store display path if it exists
            if let Some(ref display_path) = virtual_file.display_path {
                path_map.insert(display_path.clone(), virtual_file.ino);
            }
        }
    
        /// Add multiple VirtualFiles to the inode and path maps
        fn cache_virtual_files(&self, virtual_files: &[VirtualFile]) {
            let mut inode_map = self.inode_map.lock().unwrap();
            let mut path_map = self.path_map.lock().unwrap();
            for virtual_file in virtual_files {
                inode_map.insert(virtual_file.ino, virtual_file.clone());
                path_map.insert(virtual_file.virtual_path.clone(), virtual_file.ino);
                // Also store display path if it exists
                if let Some(ref display_path) = virtual_file.display_path {
                    path_map.insert(display_path.clone(), virtual_file.ino);
                }
            }
        }

    /// Initialize the filesystem by loading all virtual files
    pub async fn initialize(&self) -> Result<()> {
        info!("Initializing OneDrive FUSE filesystem...");

        // Load root directory
        let root_files = {
            let mut fuse_repo = self.fuse_repo.lock().unwrap();
            fuse_repo.list_directory("/").await?
        };

        let mut inode_map = self.inode_map.lock().unwrap();
        let mut path_map = self.path_map.lock().unwrap();

        // Add root directory
        let root_ino = 1;
        inode_map.insert(
            root_ino,
            VirtualFile {
                ino: root_ino,
                name: "/".to_string(),
                virtual_path: "/".to_string(),
                display_path: Some("/".to_string()), // Root directory uses same path for display
                parent_ino: None,
                is_folder: true,
                size: 0,
                mime_type: None,
                created_date: None,
                last_modified: None,
                content_file_id: None,
                source: FileSource::Remote,
                sync_status: None,
            },
        );
        path_map.insert("/".to_string(), root_ino);

        // Add all files and directories
        for virtual_file in root_files {
            inode_map.insert(virtual_file.ino, virtual_file.clone());
            path_map.insert(virtual_file.virtual_path.clone(), virtual_file.ino);
        }

        info!("FUSE filesystem initialized with {} items", inode_map.len());
        Ok(())
    }

    /// Get virtual file by inode (synchronous)
    fn get_virtual_file_by_ino(&self, ino: u64) -> Option<VirtualFile> {
        let inode_map = self.inode_map.lock().unwrap();
        let item = inode_map.get(&ino).cloned();
        debug!("GET_VIRTUAL_FILE_BY_INO: ino={}, item={:?}", ino, item);
        item
    }

    /// Get virtual file by path (synchronous)
    fn get_virtual_file_by_path(&self, path: &str) -> Option<VirtualFile> {
        let path_map = self.path_map.lock().unwrap();
        if let Some(&ino) = path_map.get(path) {
            self.get_virtual_file_by_ino(ino)
        } else {
            None
        }
    }

    /// Generate placeholder content for remote files
    fn generate_placeholder_content(&self, virtual_file: &VirtualFile) -> Vec<u8> {
        let placeholder = serde_json::json!({
            "onedrive_id": virtual_file.content_file_id.as_ref().unwrap_or(&"unknown".to_string()),
            "message": "remote"
        });

        serde_json::to_string_pretty(&placeholder)
            .unwrap_or_else(|_| r#"{"error": "Failed to generate placeholder"}"#.to_string())
            .into_bytes()
    }

    /// Convert VirtualFile to FUSE FileAttr
    fn virtual_file_to_attr(&self, virtual_file: &VirtualFile) -> FileAttr {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs();

        FileAttr {
            ino: virtual_file.ino,
            size: virtual_file.size,
            blocks: (virtual_file.size + 511) / 512, // 512-byte blocks
            atime: SystemTime::now(),
            mtime: SystemTime::now(),
            ctime: SystemTime::now(),
            crtime: SystemTime::now(),
            kind: if virtual_file.is_folder {
                FileType::Directory
            } else {
                FileType::RegularFile
            },
            perm: if virtual_file.is_folder { 0o755 } else { 0o644 },
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
    fn read_local_file(&self, virtual_file: &VirtualFile, offset: i64, size: u32) -> Result<Vec<u8>> {
        // Get the OneDrive ID from the virtual file
        let onedrive_id = virtual_file.content_file_id.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No OneDrive ID found for file"))?;
        
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
}

impl fuser::Filesystem for OneDriveFuse {
    fn lookup(&mut self, _req: &fuser::Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let name_str = name.to_string_lossy();
        debug!("LOOKUP: parent={}, name={}", parent, name_str);

        // Handle root directory
        if parent == 1 && name_str == "." {
            if let Some(root_file) = self.get_virtual_file_by_ino(1) {
                self.cache_virtual_file(&root_file);
                reply.entry(
                    &Duration::from_secs(1),
                    &self.virtual_file_to_attr(&root_file),
                    0,
                );
                return;
            }
        }

        // Get parent directory path
        let parent_path = if parent == 1 {
            "/".to_string()
        } else {
            if let Some(parent_file) = self.get_virtual_file_by_ino(parent) {
                self.cache_virtual_file(&parent_file);
                parent_file.virtual_path
            } else {
                reply.error(libc::ENOENT);
                return;
            }
        };

        // Construct full path
        let full_path = if parent_path == "/" {
            format!("/{}", name_str)
        } else {
            format!("{}/{}", parent_path, name_str)
        };

        // Try to get the file
        if let Some(virtual_file) = self.get_virtual_file_by_path(&full_path) {
            reply.entry(
                &Duration::from_secs(1),
                &self.virtual_file_to_attr(&virtual_file),
                0,
            );
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn getattr(&mut self, _req: &fuser::Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        debug!("GETATTR: ino={}", ino);

        if let Some(virtual_file) = self.get_virtual_file_by_ino(ino) {
            self.cache_virtual_file(&virtual_file);
            reply.attr(
                &Duration::from_secs(1),
                &self.virtual_file_to_attr(&virtual_file),
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
        let onedot_entry = (ino, fuser::FileType::Directory, ".");
        let item = self.get_virtual_file_by_ino(ino);
        if item.is_none() {
            reply.error(libc::ENOENT);
            return;
        }
        let item = item.unwrap();
        let two_dot_entry = (
            item.parent_ino.unwrap_or(1),
            fuser::FileType::Directory,
            "..",
        );
        let mut entries = vec![onedot_entry, two_dot_entry];
        let dir_items = {
            let mut fuse_repo = self.fuse_repo.lock().unwrap();
            sync_await(fuse_repo.list_directory(&item.virtual_path)).unwrap()
        };
        self.cache_virtual_files(&dir_items);
        dir_items.iter().for_each(|item| {
            entries.push((
                item.ino,
                if item.is_folder {
                    fuser::FileType::Directory
                } else {
                    fuser::FileType::RegularFile
                },
                &item.name,
            ));
            
        });
        for (i, (ino, kind, name)) in entries.into_iter().enumerate().skip(offset as usize) {
            if reply.add(ino, (i + 1) as i64, kind, name) {
                break;
            }
        }
        reply.ok();

        // // Handle root directory
        // if ino == 1 {
        //     let mut entries = vec![
        //         (1, fuser::FileType::Directory, "."),
        //         (1, fuser::FileType::Directory, ".."),
        //     ];

        //     // Get all files in root directory from inode map
        //     let inode_map = self.inode_map.lock().unwrap();
        //     for (_, virtual_file) in inode_map.iter() {
        //         if virtual_file.parent_ino == Some(1) {
        //             let file_type = if virtual_file.is_folder {
        //                 fuser::FileType::Directory
        //             } else {
        //                 fuser::FileType::RegularFile
        //             };
        //             entries.push((virtual_file.ino, file_type, &virtual_file.name));
        //         }
        //     }

        //     for (i, (ino, kind, name)) in entries.into_iter().enumerate().skip(offset as usize) {
        //         if reply.add(ino, (i + 1) as i64, kind, name) {
        //             break;
        //         }
        //     }
        //     reply.ok();
        //     return;
        // }

        // // Handle other directories
        // if let Some(parent_file) = self.get_virtual_file_by_ino(ino) {
        //     if !parent_file.is_folder {
        //         reply.error(libc::ENOTDIR);
        //         return;
        //     }

        //     let mut entries = vec![
        //         (ino, fuser::FileType::Directory, "."),
        //         (parent_file.parent_ino.unwrap_or(1), fuser::FileType::Directory, ".."),
        //     ];

        //     // Get files in this directory from inode map
        //     let inode_map = self.inode_map.lock().unwrap();
        //     for (_, virtual_file) in inode_map.iter() {
        //         if virtual_file.parent_ino == Some(ino) {
        //             let file_type = if virtual_file.is_folder {
        //                 fuser::FileType::Directory
        //             } else {
        //                 fuser::FileType::RegularFile
        //             };
        //             entries.push((virtual_file.ino, file_type, &virtual_file.name));
        //         }
        //     }

        //     for (i, (ino, kind, name)) in entries.into_iter().enumerate().skip(offset as usize) {
        //         if reply.add(ino, (i + 1) as i64, kind, name) {
        //             break;
        //         }
        //     }
        //     reply.ok();
        // } else {
        //     reply.error(libc::ENOENT);
        // }
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

        if let Some(virtual_file) = self.get_virtual_file_by_ino(ino) {
            // Get the OneDrive ID from the virtual file
            let onedrive_id = match &virtual_file.content_file_id {
                Some(id) => id,
                None => {
                    warn!("No OneDrive ID found for file: {}", virtual_file.virtual_path);
                    reply.error(libc::EIO);
                    return;
                }
            };

            // Check if this is a .onedrivedownload file (remote file with extension)
            if virtual_file.display_path.as_ref().map_or(false, |path| path.ends_with(".onedrivedownload")) {
                // Return generated placeholder content for .onedrivedownload files
                let placeholder_content = self.generate_placeholder_content(&virtual_file);
                let start = offset as usize;
                let end = std::cmp::min(start + size as usize, placeholder_content.len());

                if start < placeholder_content.len() {
                    reply.data(&placeholder_content[start..end]);
                } else {
                    reply.data(&[]);
                }
                return;
            }

            // Check if file exists in uploads directory
            if self.file_manager.file_exists_in_upload(onedrive_id) {
                let upload_path = self.file_manager.get_upload_dir().join(onedrive_id);
                match std::fs::read(&upload_path) {
                    Ok(file_data) => {
                        let start = offset as usize;
                        let end = std::cmp::min(start + size as usize, file_data.len());
                        
                        if start >= file_data.len() {
                            reply.data(&[]);
                        } else {
                            reply.data(&file_data[start..end]);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to read file from uploads {}: {}", upload_path.display(), e);
                        reply.error(libc::EIO);
                    }
                }
                return;
            }

            // Check if file exists in downloads directory
            if self.file_manager.file_exists_in_download(onedrive_id) {
                let download_path = self.file_manager.get_download_dir().join(onedrive_id);
                match std::fs::read(&download_path) {
                    Ok(file_data) => {
                        let start = offset as usize;
                        let end = std::cmp::min(start + size as usize, file_data.len());
                        
                        if start >= file_data.len() {
                            reply.data(&[]);
                        } else {
                            reply.data(&file_data[start..end]);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to read file from downloads {}: {}", download_path.display(), e);
                        reply.error(libc::EIO);
                    }
                }
                return;
            }

            // If file doesn't exist locally, return placeholder content
            let placeholder_content = self.generate_placeholder_content(&virtual_file);
            let start = offset as usize;
            let end = std::cmp::min(start + size as usize, placeholder_content.len());

            if start < placeholder_content.len() {
                reply.data(&placeholder_content[start..end]);
            } else {
                reply.data(&[]);
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
        debug!(
            "WRITE: ino={}, offset={}, data_size={}",
            ino,
            offset,
            data.len()
        );

        // For now, just acknowledge the write
        // In a real implementation, we would create a local change
        reply.written(data.len() as u32);
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

        // Create a new inode for the file
        let new_ino = 1000
            + (SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                % 10000) as u64;
        let attr = self.create_default_attr(new_ino, false);

        reply.created(&Duration::from_secs(1), &attr, 0, 0, 0);
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

        // Create a new inode for the directory
        let new_ino = 1000
            + (SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                % 10000) as u64;
        let attr = self.create_default_attr(new_ino, true);

        reply.entry(&Duration::from_secs(1), &attr, 0);
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

        // For now, just acknowledge the unlink
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

        // For now, just acknowledge the rmdir
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
        debug!(
            "RENAME: parent={}, name={}, newparent={}, newname={}",
            parent, name_str, newparent, newname_str
        );

        // For now, just acknowledge the rename
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
        _size: Option<fuser::TimeOrNow>,
        _atime: Option<fuser::TimeOrNow>,
        _mtime: Option<SystemTime>,
        _ctime: Option<u64>,
        _fh: Option<SystemTime>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<u32>,
        reply: ReplyAttr,
    ) {
        debug!("SETATTR: ino={}", ino);

        // For now, just return the current attributes
        if let Some(virtual_file) = self.get_virtual_file_by_ino(ino) {
            reply.attr(
                &Duration::from_secs(1),
                &self.virtual_file_to_attr(&virtual_file),
            );
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn statfs(&mut self, _req: &fuser::Request, _ino: u64, mut reply: ReplyStatfs) {
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
