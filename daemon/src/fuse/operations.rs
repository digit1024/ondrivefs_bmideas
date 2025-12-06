//! FUSE filesystem operations implementation

use crate::file_manager::FileManager;
use crate::fuse::attributes::AttributeManager;
use crate::fuse::drive_item_manager::DriveItemManager;
// VIRTUAL_FILE_HANDLE_ID is hardcoded as 1
use crate::fuse::filesystem::OneDriveFuse;
use crate::fuse::utils::{sync_await, FUSE_CAP_READDIRPLUS};
use crate::persistency::types::DriveItemWithFuse;
use anyhow::Context;
use fuser::{
    FileAttr, FileType, KernelConfig, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory, ReplyDirectoryPlus, ReplyEntry, ReplyStatfs, ReplyWrite, Request, TimeOrNow
};
use libc::c_int;
use log::{debug, error, info, warn};
use std::ffi::OsStr;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::fs::{File, Metadata, OpenOptions};
use libc::{O_RDONLY, O_WRONLY, O_RDWR, O_APPEND, O_CREAT, O_TRUNC, O_EXCL};


#[derive(Debug)]
pub enum AttrConversionError {
    MissingMetadata,
    InvalidData,
}

pub trait MetadataToFileAttr {
    fn try_to_file_attr(&self, ino: u64) -> Result<FileAttr, AttrConversionError>;
}

impl MetadataToFileAttr for Metadata {
    fn try_to_file_attr(&self, ino: u64) -> Result<FileAttr, AttrConversionError> {
        let kind = if self.is_dir() {
            FileType::Directory
        } else if self.is_file() {
            FileType::RegularFile
        } else if self.is_symlink() {
            FileType::Symlink
        } else {
            return Err(AttrConversionError::InvalidData);
        };

        Ok(FileAttr {
            ino,
            size: self.size(),
            blocks: (self.size() + 511) / 512,
            atime: self.accessed().map_err(|_| AttrConversionError::MissingMetadata)?,
            mtime: self.modified().map_err(|_| AttrConversionError::MissingMetadata)?,
            ctime: SystemTime::now(), // Often use current time as fallback
            crtime: self.created().unwrap_or(UNIX_EPOCH),
            kind,
            perm: (self.mode() & 0o7777) as u16, // Mask to valid permission bits
            nlink: self.nlink() as u32,
            uid: self.uid(),
            gid: self.gid(),
            rdev: self.rdev() as u32,
            blksize: self.blksize() as u32,
            flags: 0,
        })
    }
}


impl OneDriveFuse{
    pub fn get_item_by_ino(&self, ino: u64) -> DriveItemWithFuse {
        sync_await(self.database().get_item_by_ino(ino)).unwrap().unwrap()
    }
     fn read_with_handle(
        &mut self,
        fh: u64,
        offset: i64,
        size: u32,
        reply: ReplyData,
    ) {
        
        let  mut backend_file = self.file_handles().get_file(fh).unwrap().clone();
    
        match backend_file.seek(SeekFrom::Start(offset as u64)) {
            Ok(_) => {
                let mut buffer = vec![0; size as usize];
                match backend_file.read(&mut buffer) {
                    Ok(bytes_read) => {
                        reply.data(&buffer[..bytes_read]);
                    },
                    Err(e) => {
                        error!("Failed to read file data: {}", e);
                        let err_code = e.raw_os_error().unwrap_or(libc::EIO);
                        reply.error(err_code);
                    }
                }
            },
            Err(e) => {
                error!("Failed to seek file: {}", e);
                let err_code = e.raw_os_error().unwrap_or(libc::EIO);
                reply.error(err_code);
            }
        }
    }
    fn write_with_handle(
        &mut self,
        fh: u64,
        offset: i64,
        data: &[u8],
        reply: ReplyWrite,
    ) {
        let file_ptr = fh as usize as *mut std::fs::File;
        let  mut backend_file = self.file_handles().get_file(fh).unwrap().clone();
        
    
        match backend_file.seek(SeekFrom::Start(offset as u64)) {
            Ok(_) => {
                match backend_file.write_all(data) {
                    Ok(_) => {
                        // Success! Return the number of bytes written
                        reply.written(data.len() as u32);
                    },
                    Err(e) => {
                        // Write failed
                        error!("Failed to write file data: {}", e);
                        let err_code = e.raw_os_error().unwrap_or(libc::EIO);
                        reply.error(err_code);
                    }
                }
            },
            Err(e) => {
                // Seek failed
                error!("Failed to seek file for write: {}", e);
                let err_code = e.raw_os_error().unwrap_or(libc::EIO);
                reply.error(err_code);
            }
        }
    }
    // Helper method for the direct access write path
    fn write_direct(
        &mut self,
        ino: u64,
        offset: i64,
        data: &[u8],
        flags: i32,  // File status flags from open()
        reply: ReplyWrite,
    ) {
        // 1. Get file path
        let item = self.get_item_by_ino(ino);
        let file_path = match self.get_local_file_path(item.virtual_ino().unwrap_or(0)) {
            Some(path) => path,
            None => { reply.error(libc::ENOENT); return; }
        };
    
        // 2. Delegate file writing to filesystem helper
        match self.write_file_with_flags(&file_path, offset, data, flags) {
            Ok(bytes_written) => {
                // 3. Update database if needed
                if let Err(_) = sync_await(self.database().mark_db_item_as_modified(ino)) {
                    warn!("Failed to update item metadata, but write succeeded");
                }
                
                // Create processing item for file update
                if let Err(e) = self.create_processing_item(&item, crate::sync::ChangeOperation::Update) {
                    error!("Failed to create processing item for file update: {}", e);
                }
                
                reply.written(bytes_written);
            },
            Err(e) => {
                error!("Failed to write file with flags: {}", e);
                reply.error(e.raw_os_error().unwrap_or(libc::EIO))
            }
        }
    }

    // Helper for direct I/O reads (fh = 0)
    fn handle_direct_read(&mut self, ino: u64, offset: i64, size: u32, reply: ReplyData) {
        let item = match sync_await(self.database().get_item_by_ino(ino)) {
            Ok(Some(item)) => item,
            Ok(None) => { reply.error(libc::ENOENT); return; }
            Err(e) => { 
                error!("Failed to get item by ino {}: {}", ino, e);
                reply.error(libc::EIO); 
                return; 
            }
        };
        
        if item.is_folder() {
            error!("Cannot read folder as file for ino: {}", ino);
            reply.error(libc::EIO);
            return;
        }
        
        let file_path = match self.get_local_file_path(item.virtual_ino().unwrap_or(0)) {
            Some(path) => path,
            None => { reply.error(libc::ENOENT); return; }
        };
        
        match self.read_file_data(&file_path, offset as u64, size as usize) {
            Ok(data) => reply.data(&data),
            Err(e) => {
                error!("Failed to read file data from path: {}", e);
                reply.error(e.raw_os_error().unwrap_or(libc::EIO))
            }
        }
    }

    // Helper for virtual file reads (fh = 1)
    fn handle_virtual_read(&mut self, ino: u64, offset: i64, size: u32, reply: ReplyData) {
        let item = match sync_await(self.database().get_item_by_ino(ino)) {
            Ok(Some(item)) => item,
            Ok(None) => { reply.error(libc::ENOENT); return; }
            Err(e) => { 
                error!("Failed to get item by ino {}: {}", ino, e);
                reply.error(libc::EIO); 
                return; 
            }
        };
        
        if item.is_folder() {
            error!("Cannot read folder as file for ino: {}", ino);
            reply.error(libc::EIO);
            return;
        }
        
        let content = self.generate_placeholder_content(&item);
        let content_len = content.len() as i64;
        let start = offset.min(content_len);
        let end = (offset + size as i64).min(content_len);

        if start < end {
            let slice = &content[start as usize..end as usize];
            reply.data(slice);
        } else {
            reply.data(&[]); // Empty response for out-of-bounds reads
        }
    }
}

impl fuser::Filesystem for OneDriveFuse {
    fn open(&mut self, _req: &fuser::Request, ino: u64, flags: i32, reply: fuser::ReplyOpen) {
        debug!("OPEN: ino={}", ino);
        let item = self.get_item_by_ino(ino);
        let file_path = self.get_local_file_path(item.virtual_ino().unwrap_or(0));
        if file_path.is_none() {
            reply.error(libc::ENOENT);
            return;
        }
        if item.is_folder() {
            reply.error(libc::ENOENT);
            return;
        }
        let file_path = file_path.unwrap();
        
        // Use simple open options for now - we can enhance this later
        let mut open_options = OpenOptions::new();
        open_options.read(true);
        if (flags & libc::O_WRONLY) != 0 || (flags & libc::O_RDWR) != 0 {
            open_options.write(true);
        }
        if (flags & libc::O_APPEND) != 0 {
            open_options.append(true);
        }
        
        match open_options.open(&file_path) {
            Ok(backend_file) => {
                // SUCCESS: We can create a stateful session.
                let fh = self.file_handles().register_file(backend_file);
                
                reply.opened(fh, 0); // Return the valid FH
            },
            Err(e) => {
                // FAILURE: We cannot open the file (e.g., permission denied).
                // We must signal this. The kernel will then likely use
                // the direct path (fh=0) for subsequent operations, which will also fail.
                error!("Failed to open file {}: {}", file_path.display(), e);
                reply.error(e.raw_os_error().unwrap_or(libc::EIO));
            }
        }
    }

    fn release(
        &mut self,
        _req: &fuser::Request,
        ino: u64,
        fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: fuser::ReplyEmpty,
    ) {
        debug!("RELEASE: fh={}", fh);

        if fh == 0 {
            // Directory - nothing to close
            reply.ok();
            return;
        }

        // Handle virtual file handles (no cleanup needed)
        if fh == 1 {
            // VIRTUAL_FILE_HANDLE_ID
            debug!("📂 Released virtual file handle {} for inode {}", fh, ino);
            reply.ok();
            return;
        }

        // First, try to update the DriveItem if it's a file
        let update_result = {
            let item_result = sync_await(self.database().get_item_by_ino(ino));
            match item_result {
                Ok(Some(item)) => {
                    if item.is_folder() {
                        Ok(()) // No update needed for folders
                    } else {
                        // Get the file path for the item
                        if let Some(file_path) = self
                            .get_local_file_path(item.virtual_ino().unwrap_or(0))
                        {
                            // Update DriveItem with file metadata
                            let mut updated_drive_item = item.drive_item().clone();
                            match sync_await(DriveItemManager::update_drive_item_from_file(
                                &mut updated_drive_item,
                                &file_path,
                            )) {
                                Ok(_) => {
                                    // Store the updated item
                                    let mut updated_item = item.clone();
                                    *updated_item.drive_item_mut() = updated_drive_item;
                                    match sync_await(
                                        self.drive_item_with_fuse_repo()
                                            .store_drive_item_with_fuse(&updated_item),
                                    ) {
                                        Ok(_) => {
                                            debug!(
                                                "📂 Updated DriveItem metadata from file: {}",
                                                file_path.display()
                                            );
                                            Ok(())
                                        }
                                        Err(e) => {
                                            error!("Failed to store updated DriveItem: {}", e);
                                            Err(e)
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to update DriveItem from file: {}", e);
                                    Err(e)
                                }
                            }
                        } else {
                            Ok(()) // No local file found, that's okay
                        }
                    }
                }
                Ok(None) => {
                    error!("Item not found for ino: {}", ino);
                    Err(anyhow::anyhow!("Item not found"))
                }
                Err(e) => {
                    error!("Failed to get item by ino {}: {}", ino, e);
                    Err(e)
                }
            }
        };

        // Then close the file handle
        self.file_handles().close_file(fh);
    
        // When _boxed_file goes out of scope here, the File is closed!
        
        reply.ok();
 
    }



    fn lookup(&mut self, _req: &fuser::Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let name_str = name.to_string_lossy();
        debug!("LOOKUP: parent={}, name={}", parent, name_str);

        // Strip .onedrivedownload extension if present for lookup
        let lookup_name = if name_str.ends_with(".onedrivedownload") {
            &name_str[..name_str.len() - 17] // Remove ".onedrivedownload"
        } else {
            &name_str
        };

        // Use case-insensitive DB query by parent_ino and name
        if let Ok(Some(item)) = sync_await(
            self.drive_item_with_fuse_repo()
                .get_drive_item_with_fuse_by_parent_ino_and_name_case_insensitive(
                    parent,
                    lookup_name,
                ),
        ) {
            let attr = self.get_attributes_from_local_file_or_from_db(&item);
            
                reply.entry(
                    &Duration::from_secs(3),
                    &AttributeManager::item_to_file_attr(&item),
                    0,
                );
            
            
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn getattr(&mut self, _req: &fuser::Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        debug!("GETATTR: ino={}", ino);

        if let Ok(Some(item)) = sync_await(self.database().get_item_by_ino(ino)) {
            reply.attr(
                &Duration::from_secs(0),
                &self.get_attributes_from_local_file_or_from_db(&item),
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
        
        // Calculate how many dot entries we need to add
        let dots_to_add = if offset < 2 { 2 - offset as usize } else { 0 };
        
        // Add dot entries if needed
        if dots_to_add > 0 {
            let item = match sync_await(self.database().get_item_by_ino(ino)) {
                Ok(Some(item)) => item,
                Ok(None) => {
                    error!("Failed to get item by ino {}: {}", ino, "item not found");
                    reply.error(libc::ENOENT);
                    return;
                }
                Err(e) => {
                    error!("Failed to get item by ino {}: {}", ino, e);
                    reply.error(libc::EIO);
                    return;
                }
            };
            
            
            // Add "." entry
            if offset == 0 {
                
                if reply.add(
                    ino,
                    1,
                    fuser::FileType::Directory,
                    ".",
                ) {
                    reply.ok();
                    return;
                }
            }
            
            // Add ".." entry  
            if offset <= 1 {
                let parent_ino = item.parent_ino().unwrap_or(1);
                let p = self.get_item_by_ino(parent_ino);
                
                if reply.add(
                    parent_ino,
                    2,
                    fuser::FileType::Directory,
                    "..",
                ) {
                    reply.ok();
                    return;
                }
            }
        }
        
        // Calculate the actual database offset for child items
        // If offset < 2, we start from the beginning of children
        // If offset >= 2, we start from (offset - 2) in the children list
        let mut db_offset = if offset < 2 { 0 } else { (offset - 2) as usize };
        
        let batch_size = 100; // Fetch 100 items at a time
        
        loop {
            let children = match sync_await(self.database().get_children_by_parent_ino_paginated(
                ino,
                db_offset,
                batch_size,
            )) {
                Ok(children) => children,
                Err(e) => {
                    error!("Failed to get children for parent ino {}: {}", ino, e);
                    reply.error(libc::EIO);
                    return;
                }
            };

            // If no more children, we're done
            if children.is_empty() {
                break;
            }
            
            for (i, child) in children.iter().enumerate() {
                let file_type = if child.is_folder() {
                    fuser::FileType::Directory
                } else {
                    fuser::FileType::RegularFile
                };
                
                let name = if self
                    .get_local_file_path(child.virtual_ino().unwrap_or(0))
                    .is_none()
                    && !child.is_folder()
                {
                    format!("{}.onedrivedownload", child.name().unwrap_or("unknown"))
                } else {
                    child.name().unwrap_or("unknown").to_string()
                };
                

                // Calculate the entry offset for this child
                // Entry offset = db_offset + i + 3 (because dots are at 1 and 2)
                let entry_offset = (db_offset + i) as i64 + 3;
                
                // Try to add to reply buffer
                debug!("Adding entry: {} at offset {}", name, entry_offset);
                if reply.add(
                    child.virtual_ino().unwrap_or(0),
                    entry_offset,
                    file_type,
                    name,
                ) {
                    // Buffer is full, we're done
                    reply.ok();
                    return;
                }
            }
            
            // Move to next batch
            
            db_offset += children.len();
        }
        
        reply.ok();
    }

    fn read(
        &mut self,
        _req: &fuser::Request,
        ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        debug!(
            "READ: ino={}, fh={}, offset={}, size={}",
            ino, fh, offset, size
        );

        match fh {
            0 => self.handle_direct_read(ino, offset, size, reply),
            1 => self.handle_virtual_read(ino, offset, size, reply), // VIRTUAL_FILE_HANDLE_ID
            _ => self.read_with_handle(fh, offset, size, reply),
        }
    }



    fn write(
        &mut self,
        _req: &fuser::Request,
        ino: u64,
        fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        debug!(
            "WRITE: ino={}, fh={}, offset={}, size={}",
            ino,
            fh,
            offset,
            data.len()
        );
        if fh==1 {
            error!("Cannot write to virtual file handle for ino: {}", ino);
            reply.error(libc::EIO);
            return;
        }
        if fh != 0 {
            self.write_with_handle(fh, offset, data, reply);
            // Create processing item for file update after successful write
            if let Ok(Some(item)) = sync_await(self.database().get_item_by_ino(ino)) {
                if let Err(e) = self.create_processing_item(&item, crate::sync::ChangeOperation::Update) {
                    error!("Failed to create processing item for file update: {}", e);
                }
            }
        } else {
            self.write_direct(ino, offset, data, flags, reply) 
        }
    }

        fn create(
        &mut self,
        _req: &fuser::Request,
        parent: u64,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        flags: i32,
        reply: ReplyCreate,
    ) {
        let name_str = name.to_string_lossy();
        debug!("CREATE: parent={}, name={}", parent, name_str);
        
        // 1. Check for O_EXCL flag and existing file
        let create_new = (flags & libc::O_EXCL) != 0;
        if create_new && self.file_already_exists(parent, &name_str) {
            reply.error(libc::EEXIST);
            return;
        }
        
        // 2. Create file in database first to get inode
        let new_item = match sync_await(
            self.database()
                .apply_local_change_to_db_repository("create", parent, &name_str, false),
        ) {
            Ok(ino) => ino,
            Err(e) => {
                error!("Failed to create item in database: {}", e);
                reply.error(libc::EIO);
                return;
            }
        };
        
        // 3. Create physical file using helper
        let new_file_path = self.file_manager().get_local_dir().join(new_item.to_string());
        match self.create_physical_file(&new_file_path, flags) {
            Ok((backend_file, mut attr)) => {
                // Update attr with correct inode
                attr.ino = new_item;
                
                // Create file handle
                let fh = self.file_handles().register_file(backend_file);
                
                // Reply with created file info
                reply.created(&Duration::from_secs(1), &attr, 0, fh, 0);
                
                // Create processing item for new file
                if let Ok(Some(item)) = sync_await(self.database().get_item_by_ino(new_item)) {
                    if let Err(e) = self.create_processing_item(&item, crate::sync::ChangeOperation::Create) {
                        error!("Failed to create processing item for new file: {}", e);
                    }
                }
            },
            Err(e) => {
                error!("Failed to create physical file: {}", e);
                reply.error(e.raw_os_error().unwrap_or(libc::EIO));
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

        match sync_await(
            self.database()
                .apply_local_change_to_db_repository("mkdir", parent, &name_str, true),
                

        ) {
            Ok(ino) => {
                if let Ok(Some(item)) = sync_await(self.database().get_item_by_ino(ino)) {
                    


                    reply.entry(
                        &Duration::from_secs(3),
                        &AttributeManager::item_to_file_attr(&item),
                        0,
                    );
                    if let Err(e) = self.create_processing_item(&item, crate::sync::ChangeOperation::Create) {
                    }

                } else {
                    error!("Failed to get created directory item by ino: {}", ino);
                    reply.error(libc::EIO);
                }
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

        // Get the item to be deleted (case-insensitive lookup)
        if let Ok(Some(item)) = sync_await(
            self.drive_item_with_fuse_repo()
                .get_drive_item_with_fuse_by_parent_ino_and_name_case_insensitive(
                    parent, &name_str,
                ),
        ) {
            let onedrive_id = item.id();

            // Clean up any open handles for this inode
            

            // Mark as deleted in database
            let mut updated_item = item.clone();
            updated_item.drive_item_mut().mark_deleted();

            if let Err(e) = sync_await(
                self.drive_item_with_fuse_repo()
                    .store_drive_item_with_fuse(&updated_item),
            ) {
                error!("Failed to mark item as deleted: {}", e);
                reply.error(libc::EIO);
                return;
            }

            debug!("📂 Unlinked file: {} ({})", name_str, onedrive_id);
            
            // Create processing item for file deletion
            if let Err(e) = self.create_processing_item(&item, crate::sync::ChangeOperation::Delete) {
                error!("Failed to create processing item for file deletion: {}", e);
            }
            
            reply.ok();
        } else {
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
        debug!("RMDIR: parent={}, name={}", parent, name_str);

        // Get the directory to be deleted (case-insensitive lookup)
        if let Ok(Some(item)) = sync_await(
            self.drive_item_with_fuse_repo()
                .get_drive_item_with_fuse_by_parent_ino_and_name_case_insensitive(
                    parent, &name_str,
                ),
        ) {
            let onedrive_id = item.id();

            // Check if directory is empty
            if let Ok(children) = sync_await(
                self.database()
                    .get_children_by_parent_ino(item.virtual_ino().unwrap_or(0)),
            ) {
                if !children.is_empty() {
                    debug!("📂 Cannot remove non-empty directory: {}", name_str);
                    reply.error(libc::ENOTEMPTY);
                    return;
                }
            } else {
                error!("Failed to get children for directory ino {}: {}", item.virtual_ino().unwrap_or(0), "database error");
            }

            

            // Mark as deleted in database
            let mut updated_item = item.clone();
            updated_item.drive_item_mut().mark_deleted();

            if let Err(e) = sync_await(
                self.drive_item_with_fuse_repo()
                    .store_drive_item_with_fuse(&updated_item),
            ) {
                error!("Failed to mark directory as deleted: {}", e);
                reply.error(libc::EIO);
                return;
            }

            debug!("📂 Removed directory: {} ({})", name_str, onedrive_id);
            
            // Create processing item for directory deletion
            if let Err(e) = self.create_processing_item(&item, crate::sync::ChangeOperation::Delete) {
                error!("Failed to create processing item for directory deletion: {}", e);
            }
            
            reply.ok();
        } else {
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
        flags: u32,
        reply: fuser::ReplyEmpty,
    ) {
        let name_str = name.to_string_lossy();
        let newname_str = newname.to_string_lossy();
        debug!(
            "RENAME: parent={}, name={} -> newparent={}, newname={}",
            parent, name_str, newparent, newname_str
        );

        // Get the item to be renamed
        let original_item = match sync_await(
            self.drive_item_with_fuse_repo()
                .get_drive_item_with_fuse_by_parent_ino_and_name_case_insensitive(parent, &name_str)
        ) {
            Ok(Some(item)) => item,
            Ok(None) => {
                reply.error(libc::ENOENT);
                return;
            }
            Err(e) => {
                error!("Failed to get original item: {}", e);
                reply.error(libc::EIO);
                return;
            }
        };

        // Check if target already exists
        let existing_target = sync_await(
            self.drive_item_with_fuse_repo()
                .get_drive_item_with_fuse_by_parent_ino_and_name_case_insensitive(newparent, &newname_str)
        ).unwrap_or(None);

        // Handle replace operation
        if let Some(target_item) = existing_target {
            // Check RENAME_NOREPLACE flag
            if flags & libc::RENAME_NOREPLACE != 0 {
                reply.error(libc::EEXIST);
                return;
            }

            // Delegate to helper
            match self.handle_replace_operation(&original_item, &target_item) {
                Ok(_) => {
                    debug!("📂 Replaced: {} -> {}", name_str, newname_str);
                    
                    // Create processing items for replace operation:
                    // 1. DELETE for original item
                    if let Err(e) = self.create_processing_item(&original_item, crate::sync::ChangeOperation::Delete) {
                        error!("Failed to create DELETE processing item for replace: {}", e);
                    }
                    
                    // 2. UPDATE for target item (because its content changed - file moved there)
                    if let Err(e) = self.create_processing_item(&target_item, crate::sync::ChangeOperation::Update) {
                        error!("Failed to create UPDATE processing item for replace target: {}", e);
                    }
                    
                    reply.ok();
                }
                Err(e) => {
                    error!("Failed to handle replace operation: {}", e);
                    reply.error(e.raw_os_error().unwrap_or(libc::EIO));
                }
            }
            return;
        }

        // Normal rename operation - delegate to helper
        match self.rename_item_in_db(&original_item, newparent, &newname_str) {
            Ok(_) => {
                debug!("📂 Renamed: {} -> {} ({})", name_str, newname_str, original_item.id());
                let original_item = sync_await(
                    self.drive_item_with_fuse_repo()
                        .get_drive_item_with_fuse_by_parent_ino_and_name_case_insensitive(newparent, &newname_str)
                ).unwrap().unwrap();// We need to obtain Modified item to create processing item
                // Create processing item based on operation type:
                if parent != newparent {
                    // Different parent = Move operation
                    if let Err(e) = self.create_processing_item(&original_item, crate::sync::ChangeOperation::Move) {
                        error!("Failed to create MOVE processing item: {}", e);
                    }
                } else {
                    // Same parent, different name = Rename operation
                    if let Err(e) = self.create_processing_item(&original_item, crate::sync::ChangeOperation::Rename) {
                        error!("Failed to create RENAME processing item: {}", e);
                    }
                }
                
                reply.ok();
            }
            Err(e) => {
                error!("Failed to rename item: {}", e);
                reply.error(libc::EIO);
            }
        }
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
        _crtime: Option<SystemTime>,
        chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        debug!("SETATTR: ino={}", ino);
        

        if let Ok(Some(item)) = sync_await(self.database().get_item_by_ino(ino)) {
            // Mark as modified if any attributes changed
            if size.is_some() || mtime.is_some() {
                if let Err(e) = sync_await(self.database().mark_db_item_as_modified(ino)) {
                    warn!("Failed to mark item as modified: {}", e);
                }
            }
            let path = self.get_local_file_path(item.virtual_ino().unwrap_or(0));
            if path.is_some() {
                let file_path = path.unwrap();
                if let Some(new_size) = size {
                    match std::fs::OpenOptions::new().write(true).open(&file_path) {
                        Ok(file) => {
                            if let Err(e) = file.set_len(new_size) {
                                error!("Failed to truncate file {}: {}", file_path.display(), e);
                                reply.error(e.raw_os_error().unwrap_or(libc::EIO));
                                return;
                            }
                        }
                        Err(e) => {
                            error!("Failed to open file for truncation: {}", e);
                            reply.error(e.raw_os_error().unwrap_or(libc::EIO));
                            return;
                        }
                    }
                }
            
            }
 


            reply.attr(
                &Duration::from_secs(1),
                &self.get_attributes_from_local_file_or_from_db(&item)
            );
            
            // Create processing item for attribute update if any attributes changed
            if size.is_some() || mtime.is_some() {
                if let Err(e) = self.create_processing_item(&item, crate::sync::ChangeOperation::Update) {
                    error!("Failed to create processing item for attribute update: {}", e);
                }
            }
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
    fn readdirplus(
        &mut self,
        _req: &fuser::Request<'_>,
        ino: u64,
        fh: u64,
        offset: i64,
        mut reply: ReplyDirectoryPlus,
    ) {
        debug!("READDIRPLUS: ino={}, fh={}, offset={}", ino, fh, offset);
        let children = match sync_await(self.database().get_children_by_parent_ino(ino)) {
            Ok(children) => children,
            Err(e) => {
                error!("Failed to get children for parent ino {} in readdirplus: {}", ino, e);
                reply.error(libc::EIO);
                return;
            }
        };
        let mut entries = Vec::new();

        // Get the current item
        let item = match sync_await(self.database().get_item_by_ino(ino)) {
            Ok(Some(item)) => item,
            Ok(None) => {
                reply.error(libc::ENOENT);
                return;
            }
            Err(e) => {
                error!("Failed to get item by ino {} for readdirplus: {}", ino, e);
                reply.error(libc::EIO);
                return;
            }
        };

        // Add "." and ".." entries

        let dot_ino = item.virtual_ino().unwrap_or(ino);
        let dotdot_ino = item.parent_ino().unwrap_or(1);
        let parent_item = sync_await(self.database().get_item_by_ino(dotdot_ino))
            .unwrap()
            .unwrap();
        entries.push((
            dot_ino,
            fuser::FileType::Directory,
            ".".to_string(),
            AttributeManager::item_to_file_attr(&item),
            0 as u64,
        ));
        debug!("Adding . entry: {}", dot_ino);
        entries.push((
            dotdot_ino,
            fuser::FileType::Directory,
            "..".to_string(),
            AttributeManager::item_to_file_attr(&parent_item),
            0 as u64,
        ));
        debug!("Adding .. entry: {}", dotdot_ino);

        // Add child entries
        for (i, child) in children.iter().enumerate() {
            let file_type = if child.is_folder() {
                fuser::FileType::Directory
            } else {
                fuser::FileType::RegularFile
            };
            let name = if self
                .get_local_file_path(child.virtual_ino().unwrap_or(0))
                .is_none()
                && !child.is_folder()
            {
                format!("{}.onedrivedownload", child.name().unwrap_or("unknown"))
            } else {
                child.name().unwrap_or("unknown").to_string()
            };
            let attr = AttributeManager::item_to_file_attr(&child);
            entries.push((
                child.virtual_ino().unwrap_or(0),
                file_type,
                name,
                attr,
                0 as u64,
            ));
        }

        let current_offset = 1;
        // Add entries with proper offset handling
        for (i, (ino, kind, name, attr, geno)) in entries.iter().enumerate() {
            debug!("Adding entry: {} at offset {}", name, i + 1);
            if offset < i as i64 + 1 {
                if reply.add(
                    *ino,
                    i as i64 + 1,
                    name,
                    &Duration::from_secs(5),
                    &attr,
                    geno.clone(),
                ) {
                    debug!("Failed to add");

                    break;
                }
            }
        }

        reply.ok();
    }

    fn init(&mut self, _req: &fuser::Request<'_>, config: &mut KernelConfig) -> Result<(), c_int> {
        config
            .add_capabilities(FUSE_CAP_READDIRPLUS)
            .expect("Failed to add capabilities");
        Ok(())
    }
}
