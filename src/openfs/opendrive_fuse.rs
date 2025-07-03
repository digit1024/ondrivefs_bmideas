use crate::auth::onedrive_auth::OneDriveAuth;
use crate::file_manager::{DefaultFileManager, FileManager};
use crate::metadata_manager_for_files::{MetadataManagerForFiles, get_metadata_manager_singleton};
use crate::onedrive_service::onedrive_models::DriveItem;
use crate::onedrive_service::onedrive_client::OneDriveClient;
use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyBmap, ReplyCreate, ReplyData, ReplyDirectory, ReplyEmpty, ReplyEntry, ReplyLock, ReplyOpen, ReplyStatfs, ReplyWrite, ReplyXattr, Request, TimeOrNow
};
use libc::{ENOENT, ENOSYS};

use crate::helpers::path_to_inode;
use crate::openfs::models::{DirEntry, DirHanldeManager};
use log::{debug, error, info, trace};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use time::OffsetDateTime;

const TTL: Duration = Duration::from_secs(1);

/// FUSE filesystem implementation that reads from OneDrive cache
pub struct OpenDriveFuse {
    file_manager: DefaultFileManager,
    metadata_manager: &'static MetadataManagerForFiles,
    dir_handle_manager: DirHanldeManager,
    onedrive_client: OneDriveClient,
}

pub trait ToFuseAttr {
    fn to_fuse_attr(&self) -> FileAttr;
}
fn parse_time_string(time_string: &String) -> Result<SystemTime, time::error::Parse> {
    // Parse RFC 3339 format (ISO 8601)
    let datetime = OffsetDateTime::parse(
        time_string.as_str(),
        &time::format_description::well_known::Rfc3339,
    )?;
    Ok(SystemTime::from(datetime))
}

impl ToFuseAttr for DriveItem {
    fn to_fuse_attr(&self) -> FileAttr {
        let default_time_string = String::from("1960-01-01T01:00:0-Z");
        let last_modified = self
            .last_modified
            .clone()
            .unwrap_or(default_time_string.clone());
        let created_date = self
            .created_date
            .clone()
            .unwrap_or(default_time_string.clone());

        let attr = FileAttr {
            ino: self.id.parse::<u64>().unwrap_or(1),
            size: self.size.unwrap_or(0),
            blocks: (self.size.unwrap_or(0) + 511) / 512,
            // example date is 2014-10-22T09:36:06Z
            atime: parse_time_string(&default_time_string.clone()).unwrap_or(UNIX_EPOCH),
            mtime: parse_time_string(&last_modified).unwrap_or(UNIX_EPOCH),
            ctime: parse_time_string(&last_modified).unwrap_or(UNIX_EPOCH),
            crtime: parse_time_string(&created_date).unwrap_or(UNIX_EPOCH),
            gid: 1000,
            rdev: 0,
            blksize: 512,
            flags: 0,

            kind: if self.file.is_some() {
                FileType::RegularFile
            } else {
                FileType::Directory
            },
            perm: if self.file.is_some() { 0o644 } else { 0o755 },
            nlink: 1,
            uid: 1000,
        };
        attr
    }
}

impl OpenDriveFuse {
    pub fn new(file_manager: DefaultFileManager, onedrive_client: OneDriveClient) -> Self {
        let dir_handle_manager = DirHanldeManager::new();
        let metadata_manager = get_metadata_manager_singleton();
        OpenDriveFuse {
            file_manager,
            metadata_manager,
            dir_handle_manager,
            onedrive_client,
        }
    }

    /// Read DriveItem from a cache file
    fn read_drive_item_from_cache(&self, cache_path: &Path) -> Option<DriveItem> {
        match fs::read_to_string(cache_path) {
            Ok(content) => match serde_json::from_str::<DriveItem>(&content) {
                Ok(item) => Some(item),
                Err(e) => {
                    error!(
                        "Failed to parse DriveItem from {}: {}",
                        cache_path.display(),
                        e
                    );
                    None
                }
            },
            Err(e) => {
                debug!("Failed to read cache file {}: {}", cache_path.display(), e);
                None
            }
        }
    }

    /// Get cache path for a virtual path
    fn virtual_path_to_cache_path(&self, virtual_path: &Path) -> PathBuf {
        if virtual_path == Path::new("/") {
            // Root directory
            self.file_manager.get_cache_dir()
        } else {
            // Remove leading slash and join with cache dir
            let relative_path = virtual_path.strip_prefix("/").unwrap_or(virtual_path);
            self.file_manager.get_cache_dir().join(relative_path)
        }
    }

    /// Get FileAttr for a virtual path by reading from cache
    fn get_file_attr_from_cache(&self, virtual_path: &Path) -> Option<FileAttr> {
        trace!(
            "get_file_attr_from_cache called with virtual_path: {:?}",
            virtual_path
        );
        let cache_path = self.virtual_path_to_cache_path(virtual_path);
        trace!("get_file_attr_from_cache - cache_path: {:?}", cache_path);

        // Generate inode from virtual path
        let ino = if virtual_path == Path::new("/") {
            1 // Root directory always has inode 1
        } else {
            path_to_inode(&cache_path)
        };
        trace!("get_file_attr_from_cache - generated ino: {}", ino);

        // Try to read directory metadata first
        if virtual_path == Path::new("/") {
            // Root directory - check for .dir.json in cache root
            let dir_json_path = cache_path.join(".dir.json");
            trace!(
                "get_file_attr_from_cache - checking root .dir.json at: {:?}",
                dir_json_path
            );
            if let Some(drive_item) = self.read_drive_item_from_cache(&dir_json_path) {
                let mut attr = drive_item.to_fuse_attr();
                attr.ino = ino;
                trace!("get_file_attr_from_cache - found root .dir.json, returning attr");
                return Some(attr);
            }
        } else if cache_path.is_dir() {
            // Directory - check for .dir.json
            let dir_json_path = cache_path.join(".dir.json");
            trace!(
                "get_file_attr_from_cache - checking directory .dir.json at: {:?}",
                dir_json_path
            );
            if let Some(drive_item) = self.read_drive_item_from_cache(&dir_json_path) {
                let mut attr = drive_item.to_fuse_attr();
                attr.ino = ino;
                trace!("get_file_attr_from_cache - found directory .dir.json, returning attr");
                return Some(attr);
            }
        } else {
            // File - read metadata directly
            trace!(
                "get_file_attr_from_cache - checking file at: {:?}",
                cache_path
            );
            if let Some(drive_item) = self.read_drive_item_from_cache(&cache_path) {
                let mut attr = drive_item.to_fuse_attr();
                attr.ino = ino;
                trace!("get_file_attr_from_cache - found file, returning attr");
                return Some(attr);
            }
        }

        trace!("get_file_attr_from_cache - no metadata found, returning None");
        None
    }

    /// Read directory entries from cache
    fn read_directory_from_cache(&self, virtual_path: &Path) -> Vec<(String, FileAttr)> {
        let cache_path = self.virtual_path_to_cache_path(virtual_path);
        let mut entries = Vec::new();

        if !cache_path.is_dir() {
            return entries;
        }

        match fs::read_dir(&cache_path) {
            Ok(dir_entries) => {
                for entry in dir_entries {
                    if let Ok(entry) = entry {
                        let file_name = entry.file_name().to_string_lossy().to_string();

                        // Skip .dir.json files - they're metadata, not actual entries
                        if file_name == ".dir.json" {
                            continue;
                        }

                        // Construct virtual path for this entry
                        let child_virtual_path = if virtual_path == Path::new("/") {
                            PathBuf::from("/").join(&file_name)
                        } else {
                            virtual_path.join(&file_name)
                        };

                        // Get attributes for this entry
                        if let Some(attr) = self.get_file_attr_from_cache(&child_virtual_path) {
                            entries.push((file_name, attr));
                        }
                    }
                }
            }
            Err(e) => {
                error!("Failed to read directory {}: {}", cache_path.display(), e);
            }
        }

        entries
    }

    #[allow(dead_code)]
    fn virtual_path_from_inode(&self, _ino: u64) -> Option<PathBuf> {
        // This is a placeholder implementation
        // In a real implementation, you would maintain a mapping from inode to path
        None
    }

    #[allow(dead_code)]
    fn create_file_attr(&self, ino: u64, size: u64, is_dir: bool) -> FileAttr {
        let now = SystemTime::now();
        FileAttr {
            ino,
            size,
            blocks: (size + 511) / 512,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            kind: if is_dir { FileType::Directory } else { FileType::RegularFile },
            perm: if is_dir { 0o755 } else { 0o644 },
            nlink: if is_dir { 2 } else { 1 },
            uid: 1000,
            gid: 1000,
            rdev: 0,
            flags: 0,
            blksize: 512,
        }
    }

    /// Check if a file exists in the temp download directory
    fn file_exists_in_temp(&self, virtual_path: &Path) -> bool {
        let temp_path = self.file_manager.virtual_path_to_downloaded_path(virtual_path);
        temp_path.exists() && temp_path.is_file()
    }

    /// Get the temp download path for a virtual path
    fn get_temp_path_for_virtual_path(&self, virtual_path: &Path) -> PathBuf {
        self.file_manager.virtual_path_to_downloaded_path(virtual_path)
    }

    /// Download a file on demand if it doesn't exist in temp
    async fn ensure_file_downloaded(&mut self, virtual_path: &Path) -> Result<(), libc::c_int> {
        // Skip if it's a directory
        if virtual_path == Path::new("/") {
            return Ok(());
        }

        // Check if file already exists in temp
        if self.file_exists_in_temp(virtual_path) {
            return Ok(());
        }

        // Get the DriveItem from cache to get download URL
        let cache_path = self.virtual_path_to_cache_path(virtual_path);
        let drive_item = match self.read_drive_item_from_cache(&cache_path) {
            Some(item) => item,
            None => {
                error!("No DriveItem found in cache for path: {:?}", virtual_path);
                return Err(ENOENT);
            }
        };

        // Check if it's a file and has a download URL
        if drive_item.file.is_none() {
            // It's a directory, no need to download
            return Ok(());
        }

        let _download_url = match drive_item.download_url {
            Some(url) => url,
            None => {
                error!("No download URL for file: {:?}", virtual_path);
                return Err(ENOENT);
            }
        };

        // Download the file
        let file_name = match drive_item.name {
            Some(name) => name,
            None => {
                error!("No file name for file: {:?}", virtual_path);
                return Err(ENOENT);
            }
        };

        info!("Downloading file on demand: {}", virtual_path.display());
        let temp_inode = self.onedrive_client.get_item_by_id(&drive_item.id).await.unwrap();
        let download_url = temp_inode.download_url;
        if download_url.is_none() {
            error!("No download URL for file: {:?}", virtual_path);
            return Err(ENOENT);
        }

        match self.onedrive_client.download_file(&download_url.unwrap(), &drive_item.id, &file_name).await {
            Ok(download_result) => {
                let temp_path = self.get_temp_path_for_virtual_path(virtual_path);
                
                // Create parent directory if it doesn't exist
                if let Some(parent) = temp_path.parent() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        error!("Failed to create parent directory for {}: {}", temp_path.display(), e);
                        return Err(libc::EIO);
                    }
                }

                // Save the downloaded file
                match std::fs::write(&temp_path, &download_result.file_data) {
                    Ok(_) => {
                        info!("Successfully downloaded file: {}", temp_path.display());
                        Ok(())
                    }
                    Err(e) => {
                        error!("Failed to save downloaded file {}: {}", temp_path.display(), e);
                        Err(libc::EIO)
                    }
                }
            }
            Err(e) => {
                error!("Failed to download file {}: {}", virtual_path.display(), e);
                Err(libc::EIO)
            }
        }
    }
}

#[allow(unused_variables)]
impl Filesystem for OpenDriveFuse {
    fn init(
        &mut self,
        _req: &Request<'_>,
        _config: &mut fuser::KernelConfig,
    ) -> Result<(), libc::c_int> {
        info!("OpenDriveFuse: filesystem initialized");
        Ok(())
    }

    fn destroy(&mut self) {
        info!("OpenDriveFuse: filesystem destroyed");
    }

    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        debug!("OpenDriveFuse: lookup parent={}, name={:?}", parent, name);

        let name_str = name.to_string_lossy();

        let child_path = if parent == 1 {
            PathBuf::from("/").join(name_str.as_ref())
        } else {
            let path = self
                .metadata_manager
                .get_local_path_for_inode(parent)
                .unwrap()
                .unwrap();
            trace!("lookup - cache path from inode {}: {}", parent, path);
            let virtual_path = self
                .file_manager
                .cache_path_to_virtual_path(Path::new(&path));
            trace!("lookup - virtual path: {:?}", virtual_path);
            let child_path = virtual_path.join(name_str.as_ref());
            trace!("lookup - child_path: {:?}", child_path);
            child_path
        };

        trace!("lookup - final child_path: {:?}", child_path);
        if let Some(attr) = self.get_file_attr_from_cache(&child_path) {
            trace!("lookup - found attr for child_path, ino: {}", attr.ino);
            reply.entry(&TTL, &attr, 0);
            return;
        }

        trace!("lookup - no attr found for child_path");
        reply.error(ENOENT);
    }

    fn forget(&mut self, _req: &Request<'_>, ino: u64, nlookup: u64) {
        debug!("OpenDriveFuse: forget ino={}, nlookup={}", ino, nlookup);
        // Stub implementation - no action needed
    }

    fn getattr(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyAttr) {
        debug!("OpenDriveFuse: getattr ino={}", ino);

        // Try to load from file cache based on inode
        if ino == 1 {
            // Root directory
            trace!("getattr - handling root directory");
            if let Some(attr) = self.get_file_attr_from_cache(&PathBuf::from("/")) {
                reply.attr(&TTL, &attr);
                return;
            }
        } else {
            // For other inodes, we'd need to resolve the inode to a path
            trace!("getattr - resolving inode {} to path", ino);
            if let Some(cache_path) = self.metadata_manager.get_local_path_for_inode(ino).unwrap() {
                trace!("getattr - cache path from inode: {}", cache_path);
                let virtual_path = self
                    .file_manager
                    .cache_path_to_virtual_path(Path::new(&cache_path));
                trace!("getattr - virtual path: {:?}", virtual_path);
                if let Some(attr) = self.get_file_attr_from_cache(&virtual_path) {
                    reply.attr(&TTL, &attr);
                    return;
                }
            } else {
                trace!("getattr - no cache path found for inode {}", ino);
            }
        }

        trace!("getattr - returning ENOENT for ino {}", ino);
        reply.error(ENOENT);
    }

    fn setattr(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        atime: Option<TimeOrNow>,
        mtime: Option<TimeOrNow>,
        ctime: Option<SystemTime>,
        fh: Option<u64>,
        crtime: Option<SystemTime>,
        chgtime: Option<SystemTime>,
        bkuptime: Option<SystemTime>,
        flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        debug!("OpenDriveFuse: setattr ino={}", ino);

        // For now, setattr is not fully implemented for cache-based file system
        // We would need to modify the cache files and OneDrive data
        // Return ENOSYS to indicate this operation is not supported
        reply.error(ENOSYS);
    }

    fn readlink(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyData) {
        debug!("OpenDriveFuse: readlink ino={}", ino);
        reply.error(ENOSYS);
    }

    fn mknod(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        mode: u32,
        umask: u32,
        rdev: u32,
        reply: ReplyEntry,
    ) {
        debug!("OpenDriveFuse: mknod parent={}, name={:?}", parent, name);
        reply.error(ENOSYS);
    }

    fn mkdir(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        mode: u32,
        umask: u32,
        reply: ReplyEntry,
    ) {
        debug!("OpenDriveFuse: mkdir parent={}, name={:?}", parent, name);

        // mkdir is not supported for read-only cache filesystem
        // In a full implementation, this would create a directory in OneDrive
        reply.error(ENOSYS);
    }

    fn unlink(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        debug!("OpenDriveFuse: unlink parent={}, name={:?}", parent, name);
        reply.ok();
    }

    fn rmdir(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        debug!("OpenDriveFuse: rmdir parent={}, name={:?}", parent, name);
        reply.ok();
    }

    fn symlink(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        link: &std::path::Path,
        reply: ReplyEntry,
    ) {
        debug!(
            "OpenDriveFuse: symlink parent={}, name={:?}, link={:?}",
            parent, name, link
        );
        reply.error(ENOSYS);
    }

    fn rename(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        newparent: u64,
        newname: &OsStr,
        flags: u32,
        reply: ReplyEmpty,
    ) {
        debug!(
            "OpenDriveFuse: rename parent={}, name={:?}, newparent={}, newname={:?}",
            parent, name, newparent, newname
        );
        reply.ok();
    }

    fn link(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        newparent: u64,
        newname: &OsStr,
        reply: ReplyEntry,
    ) {
        debug!(
            "OpenDriveFuse: link ino={}, newparent={}, newname={:?}",
            ino, newparent, newname
        );
        reply.error(ENOSYS);
    }

    fn open(&mut self, _req: &Request<'_>, ino: u64, flags: i32, reply: ReplyOpen) {
        debug!("OpenDriveFuse: open ino={}, flags={}", ino, flags);

        // Check if file exists by trying to get its attributes from cache
        if ino == 1 {
            // Root directory
            if self.get_file_attr_from_cache(&PathBuf::from("/")).is_some() {
                reply.opened(0, 0);
            } else {
                reply.error(ENOENT);
            }
        } else {
            // For other files, try to resolve the inode to a path
            if let Some(cache_path) = self.metadata_manager.get_local_path_for_inode(ino).unwrap() {
                let virtual_path = self.file_manager.cache_path_to_virtual_path(Path::new(&cache_path));
                
                // Check if it's a file (not a directory)
                if let Some(attr) = self.get_file_attr_from_cache(&virtual_path) {
                    if attr.kind == FileType::RegularFile {
                        // For files, ensure they are downloaded
                        let runtime = tokio::runtime::Handle::current();
                        match runtime.block_on(self.ensure_file_downloaded(&virtual_path)) {
                            Ok(_) => {
                                info!("OpenDriveFuse: opened file ino={}, path={:?}", ino, virtual_path);
                                reply.opened(0, 0);
                            }
                            Err(e) => {
                                error!("OpenDriveFuse: failed to ensure file downloaded for ino={}: {}", ino, e);
                                reply.error(e);
                            }
                        }
                    } else {
                        // It's a directory
                        info!("OpenDriveFuse: opened directory ino={}, path={:?}", ino, virtual_path);
                        reply.opened(0, 0);
                    }
                } else {
                    reply.error(ENOENT);
                }
            } else {
                reply.error(ENOENT);
            }
        }
    }

    fn read(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        flags: i32,
        lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        debug!(
            "OpenDriveFuse: read ino={}, offset={}, size={}",
            ino, offset, size
        );

        // Skip for root directory
        if ino == 1 {
            reply.error(ENOENT);
            return;
        }

        // Resolve inode to virtual path
        let virtual_path = match self.metadata_manager.get_local_path_for_inode(ino).unwrap() {
            Some(cache_path) => self.file_manager.cache_path_to_virtual_path(Path::new(&cache_path)),
            None => {
                error!("No path found for inode {}", ino);
                reply.error(ENOENT);
                return;
            }
        };

        // Get the temp file path
        let temp_path = self.get_temp_path_for_virtual_path(&virtual_path);

        // Check if file exists in temp
        if !self.file_exists_in_temp(&virtual_path) {
            error!("File not found in temp directory: {}", temp_path.display());
            reply.error(ENOENT);
            return;
        }

        // Read file data using standard filesystem calls
        match std::fs::read(&temp_path) {
            Ok(file_data) => {
                let file_size = file_data.len() as i64;
                
                // Handle offset
                if offset >= file_size {
                    // Requested offset is beyond file size, return empty data
                    reply.data(&[]);
                    return;
                }

                // Calculate the actual read size
                let start = offset as usize;
                let end = std::cmp::min(start + size as usize, file_data.len());
                let read_data = &file_data[start..end];

                debug!(
                    "Read {} bytes from file {} (offset={}, requested_size={})",
                    read_data.len(),
                    temp_path.display(),
                    offset,
                    size
                );

                reply.data(read_data);
            }
            Err(e) => {
                error!("Failed to read file {}: {}", temp_path.display(), e);
                reply.error(libc::EIO);
            }
        }
    }

    fn write(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        offset: i64,
        data: &[u8],
        write_flags: u32,
        flags: i32,
        lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        debug!(
            "OpenDriveFuse: write ino={}, offset={}, size={}",
            ino,
            offset,
            data.len()
        );

        // Stub implementation - pretend to write all data
        reply.written(data.len() as u32);
    }

    fn flush(&mut self, _req: &Request<'_>, ino: u64, fh: u64, lock_owner: u64, reply: ReplyEmpty) {
        debug!("OpenDriveFuse: flush ino={}", ino);
        reply.ok();
    }

    fn release(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        flags: i32,
        lock_owner: Option<u64>,
        flush: bool,
        reply: ReplyEmpty,
    ) {
        debug!("OpenDriveFuse: release ino={}", ino);
        reply.ok();
    }

    fn fsync(&mut self, _req: &Request<'_>, ino: u64, fh: u64, datasync: bool, reply: ReplyEmpty) {
        debug!("OpenDriveFuse: fsync ino={}", ino);
        reply.ok();
    }

    fn opendir(&mut self, _req: &Request<'_>, ino: u64, flags: i32, reply: ReplyOpen) {
        trace!("opendir called with ino={}", ino);
        let dir_handle = self.dir_handle_manager.new_dir_handle();
        let entries = if ino == 1 {
            trace!("opendir - reading root directory");
            self.read_directory_from_cache(&PathBuf::from("/"))
        } else {
            let path = self
                .metadata_manager
                .get_local_path_for_inode(ino)
                .unwrap()
                .unwrap();
            trace!("opendir - cache path from inode {}: {}", ino, path);
            let virtual_path = self
                .file_manager
                .cache_path_to_virtual_path(Path::new(&path));
            trace!("opendir - virtual path: {:?}", virtual_path);
            let entries = self.read_directory_from_cache(&virtual_path);
            trace!("opendir - found {} entries", entries.len());
            entries
        };

        trace!("opendir - processing {} entries", entries.len());
        let mut current_offset = 0;
        self.dir_handle_manager.append_to_handle(
            dir_handle,
            DirEntry::new(1, 0, FileType::Directory, ".".to_string()),
        );
        self.dir_handle_manager.append_to_handle(
            dir_handle,
            DirEntry::new(1, 1, FileType::Directory, "..".to_string()),
        );
        current_offset += 2;
        for (name, attr) in entries.iter() {
            trace!(
                "opendir - adding entry: name={}, ino={}, kind={:?}",
                name, attr.ino, attr.kind
            );
            let entry = DirEntry::new(attr.ino, current_offset, attr.kind, name.clone());
            if attr.ino == 0 {
                trace!("opendir - skipping entry with ino=0: {}", name);
                continue;
            }
            self.dir_handle_manager.append_to_handle(dir_handle, entry);
            current_offset += 1;
        }

        info!(
            "OpenDriveFuse: opendir ino={}, dir_handle={}",
            ino, dir_handle
        );
        reply.opened(dir_handle, 0);
    }

    fn readdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        debug!("OpenDriveFuse: readdir ino={}, offset={}", ino, offset);

        let entries = self.dir_handle_manager.get_dir_handle(fh).unwrap();
        if offset >= entries.len() as i64 - 1 {
            reply.ok();
            return;
        }
        for entry in entries.iter().skip(offset as usize) {
            let _ = reply.add(entry.ino, entry.offset, entry.kind, entry.name.clone());
        }

        reply.ok();
        return;
    }

    fn releasedir(&mut self, _req: &Request<'_>, ino: u64, fh: u64, flags: i32, reply: ReplyEmpty) {
        self.dir_handle_manager.remove_dir_handle(fh);
        reply.ok();
    }

    fn fsyncdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        datasync: bool,
        reply: ReplyEmpty,
    ) {
        debug!("OpenDriveFuse: fsyncdir ino={}", ino);
        reply.ok();
    }

    fn statfs(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyStatfs) {
        debug!("OpenDriveFuse: statfs ino={}", ino);

        // Stub implementation with dummy values
        reply.statfs(
            1024 * 1024, // blocks
            1024 * 512,  // bfree
            1024 * 512,  // bavail
            1024,        // files
            512,         // ffree
            512,         // bsize
            255,         // namelen
            512,         // frsize
        );
    }

    fn setxattr(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        name: &OsStr,
        value: &[u8],
        flags: i32,
        position: u32,
        reply: ReplyEmpty,
    ) {
        debug!("OpenDriveFuse: setxattr ino={}, name={:?}", ino, name);
        reply.error(ENOSYS);
    }

    fn getxattr(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        name: &OsStr,
        size: u32,
        reply: ReplyXattr,
    ) {
        debug!("OpenDriveFuse: getxattr ino={}, name={:?}", ino, name);
        reply.error(ENOSYS);
    }

    fn listxattr(&mut self, _req: &Request<'_>, ino: u64, size: u32, reply: ReplyXattr) {
        debug!("OpenDriveFuse: listxattr ino={}", ino);
        reply.error(ENOSYS);
    }

    fn removexattr(&mut self, _req: &Request<'_>, ino: u64, name: &OsStr, reply: ReplyEmpty) {
        debug!("OpenDriveFuse: removexattr ino={}, name={:?}", ino, name);
        reply.error(ENOSYS);
    }

    fn access(&mut self, _req: &Request<'_>, ino: u64, mask: i32, reply: ReplyEmpty) {
        debug!("OpenDriveFuse: access ino={}, mask={}", ino, mask);
        reply.ok();
    }

    fn create(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        mode: u32,
        umask: u32,
        flags: i32,
        reply: ReplyCreate,
    ) {
        debug!("OpenDriveFuse: create parent={}, name={:?}", parent, name);

        // create is not supported for read-only cache filesystem
        // In a full implementation, this would create a file in OneDrive
        reply.error(ENOSYS);
    }

    fn getlk(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        lock_owner: u64,
        start: u64,
        end: u64,
        typ: i32,
        pid: u32,
        reply: ReplyLock,
    ) {
        debug!("OpenDriveFuse: getlk ino={}", ino);
        reply.error(ENOSYS);
    }

    fn setlk(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        lock_owner: u64,
        start: u64,
        end: u64,
        typ: i32,
        pid: u32,
        sleep: bool,
        reply: ReplyEmpty,
    ) {
        debug!("OpenDriveFuse: setlk ino={}", ino);
        reply.error(ENOSYS);
    }

    fn bmap(&mut self, _req: &Request<'_>, ino: u64, blocksize: u32, idx: u64, reply: ReplyBmap) {
        debug!("OpenDriveFuse: bmap ino={}", ino);
        reply.error(ENOSYS);
    }

    fn ioctl(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        flags: u32,
        cmd: u32,
        in_data: &[u8],
        out_size: u32,
        reply: fuser::ReplyIoctl,
    ) {
        debug!("OpenDriveFuse: ioctl ino={}", ino);
        reply.error(ENOSYS);
    }

    fn fallocate(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        offset: i64,
        length: i64,
        mode: i32,
        reply: ReplyEmpty,
    ) {
        debug!("OpenDriveFuse: fallocate ino={}", ino);
        reply.error(ENOSYS);
    }

    fn readdirplus(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        offset: i64,
        mut reply: fuser::ReplyDirectoryPlus,
    ) {
        debug!("OpenDriveFuse: readdirplus ino={}, offset={}", ino, offset);

        if ino == 1 {
            // Root directory
            if offset == 0 {
                if let Some(attr) = self.get_file_attr_from_cache(&PathBuf::from("/")) {
                    reply.add(1, 0, ".", &TTL, &attr, 0);
                    reply.add(1, 1, "..", &TTL, &attr, 0);
                }
            }
        }
        reply.ok();
    }

    fn lseek(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        offset: i64,
        whence: i32,
        reply: fuser::ReplyLseek,
    ) {
        debug!(
            "OpenDriveFuse: lseek ino={}, offset={}, whence={}",
            ino, offset, whence
        );
        reply.error(ENOSYS);
    }

    fn copy_file_range(
        &mut self,
        _req: &Request<'_>,
        ino_in: u64,
        fh_in: u64,
        offset_in: i64,
        ino_out: u64,
        fh_out: u64,
        offset_out: i64,
        len: u64,
        flags: u32,
        reply: ReplyWrite,
    ) {
        debug!(
            "OpenDriveFuse: copy_file_range ino_in={}, ino_out={}",
            ino_in, ino_out
        );
        reply.error(ENOSYS);
    }
}

/// Mount the FUSE filesystem
pub fn mount_filesystem(mountpoint: &str) -> anyhow::Result<()> {
    let file_manager = tokio::runtime::Handle::current().block_on(DefaultFileManager::new())?;
    
    // Initialize OneDrive authentication
    let auth = Arc::new(OneDriveAuth::new()?);
    let onedrive_client = OneDriveClient::new(auth)?;

    let fs = OpenDriveFuse::new(file_manager, onedrive_client);
    let options = vec![
        MountOption::RW,
        MountOption::FSName("opendrive".to_string()),
    ];

    info!("Mounting OpenDrive FUSE filesystem at: {}", mountpoint);
    fuser::mount2(fs, mountpoint, &options)
        .map_err(|e| anyhow::anyhow!("Failed to mount filesystem: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    async fn create_test_file_manager() -> DefaultFileManager {
        // Create a temporary directory for testing
        let temp_dir = tempdir().unwrap();
        unsafe {
            std::env::set_var("HOME", temp_dir.path());
        }
        DefaultFileManager::new().await.unwrap()
    }

    #[tokio::test]
    async fn test_file_exists_in_temp() {
        let file_manager = create_test_file_manager().await;
        let auth = Arc::new(OneDriveAuth::new().unwrap());
        let onedrive_client = OneDriveClient::new(auth).unwrap();
        let fuse = OpenDriveFuse::new(file_manager, onedrive_client);

        // Test with root path
        let root_path = PathBuf::from("/");
        assert!(!fuse.file_exists_in_temp(&root_path));

        // Test with non-existent file
        let test_path = PathBuf::from("/test.txt");
        assert!(!fuse.file_exists_in_temp(&test_path));
    }

    #[tokio::test]
    async fn test_get_temp_path_for_virtual_path() {
        let file_manager = create_test_file_manager().await;
        let auth = Arc::new(OneDriveAuth::new().unwrap());
        let onedrive_client = OneDriveClient::new(auth).unwrap();
        let fuse = OpenDriveFuse::new(file_manager, onedrive_client);

        let virtual_path = PathBuf::from("/test.txt");
        let temp_path = fuse.get_temp_path_for_virtual_path(&virtual_path);
        
        // Should convert virtual path to temp download path
        assert!(temp_path.to_string_lossy().contains("tmp/downloads"));
        assert!(temp_path.to_string_lossy().ends_with("test.txt"));
    }
}
