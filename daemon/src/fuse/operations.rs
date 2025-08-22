//! FUSE filesystem operations implementation

use crate::file_manager::FileManager;
use crate::fuse::attributes::AttributeManager;
use crate::fuse::drive_item_manager::DriveItemManager;
use crate::fuse::file_handles::VIRTUAL_FILE_HANDLE_ID;
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
use std::fs::{Metadata, OpenOptions};
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

// A struct to hold the parsed open options
#[derive(Debug, Default)]
struct OpenFlags {
    pub read: bool,
    pub write: bool,
    pub append: bool,
    pub create: bool,
    pub truncate: bool,
    pub create_new: bool,
}
impl OpenFlags {
    fn from_i32(flags: i32) -> Result<Self, i32> {
        let mut config = OpenFlags::default();
        
        let access_mode = flags & libc::O_ACCMODE;
        match access_mode {
            O_RDONLY => config.read = true,
            O_WRONLY => config.write = true,
            O_RDWR => {
                config.read = true;
                config.write = true;
            },
            _ => return Err(libc::EINVAL), // Invalid access mode
        }
        
        // Set other flags
        config.append = (flags & O_APPEND) != 0;
        config.create = (flags & O_CREAT) != 0;
        config.truncate = (flags & O_TRUNC) != 0;
        config.create_new = (flags & O_EXCL) != 0;
        
        Ok(config)
    }
    
fn apply_to<'a>(&self, options: &'a mut OpenOptions) -> &'a mut OpenOptions {
    options
        .read(self.read)    
        .write(self.write)
        .append(self.append)
        .create(self.create)
        .truncate(self.truncate)
        .create_new(self.create_new)
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
        let file_ptr = fh as usize as *mut std::fs::File;
        
        let backend_file = unsafe { &mut *file_ptr };
    
        match backend_file.seek(SeekFrom::Start(offset as u64)) {
            Ok(_) => {
                let mut buffer = vec![0; size as usize];
                match backend_file.read(&mut buffer) {
                    Ok(bytes_read) => {
                        reply.data(&buffer[..bytes_read]);
                    },
                    Err(e) => {
                        let err_code = e.raw_os_error().unwrap_or(libc::EIO);
                        reply.error(err_code);
                    }
                }
            },
            Err(e) => {
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
        let backend_file = unsafe { &mut *file_ptr };
        
    
        match backend_file.seek(SeekFrom::Start(offset as u64)) {
            Ok(_) => {
                match backend_file.write_all(data) {
                    Ok(_) => {
                        // Success! Return the number of bytes written
                        reply.written(data.len() as u32);
                    },
                    Err(e) => {
                        // Write failed
                        let err_code = e.raw_os_error().unwrap_or(libc::EIO);
                        reply.error(err_code);
                    }
                }
            },
            Err(e) => {
                // Seek failed
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
        let item = self.get_item_by_ino(ino);
        let file_path = self.file_operations().file_exists_locally(item.virtual_ino().unwrap_or(0));
        if file_path.is_none() {
            reply.error(libc::ENOENT);
            return;
        }
        let file_path = file_path.unwrap();
    
        // 2. Parse the flags to understand how the file was opened
        let open_flags = match OpenFlags::from_i32(flags) {
            Ok(flags) => flags,
            Err(e) => {
                reply.error(e);
                return;
            }
        };
    
        // 3. Handle O_APPEND mode - ignore provided offset and seek to end
        let actual_offset = if open_flags.append {
            // In append mode, we need to get the current file size
            match std::fs::metadata(&file_path) {
                Ok(metadata) => metadata.len() as i64,
                Err(e) => {
                    reply.error(e.raw_os_error().unwrap_or(libc::EIO));
                    return;
                }
            }
        } else {
            offset // Use the provided offset
        };
    
        // 4. Configure open options based on the flags
        let mut open_options = OpenOptions::new();
        open_flags.apply_to(&mut open_options);
    
        // 5. For direct writes, we need special handling:
        // - If writing anywhere but the end, we need read access to preserve existing data
        // - If it's a new file (offset == 0), we can write directly
        if actual_offset > 0 && !open_flags.append {
            // We're writing in the middle of the file, need read access to preserve data
            open_options.read(true);
        }
    
        // 6. Open the file and perform the write
        match open_options.open(&file_path) {
            Ok(mut backend_file) => {
                // Seek to the correct position
                match backend_file.seek(SeekFrom::Start(actual_offset as u64)) {
                    Ok(_) => {
                        // Perform the actual write
                        match backend_file.write_all(data) {
                            Ok(_) => {
                                // Optional: Flush to ensure data is on disk
                                if open_flags.write && (flags & libc::O_SYNC) != 0 {
                                    if let Err(e) = backend_file.sync_all() {
                                        eprintln!("Warning: sync failed after O_SYNC write: {}", e);
                                    }
                                }
                                
                                
                                
                                reply.written(data.len() as u32);
                            },
                            Err(e) => {
                                let err_code = e.raw_os_error().unwrap_or(libc::EIO);
                                reply.error(err_code);
                            }
                        }
                    },
                    Err(e) => {
                        let err_code = e.raw_os_error().unwrap_or(libc::EIO);
                        reply.error(err_code);
                    }
                }
                // File is automatically closed here when 'backend_file' goes out of scope
            },
            Err(e) => {
                let err_code = e.raw_os_error().unwrap_or(libc::EIO);
                reply.error(err_code);
            }
        }
    }
}

impl fuser::Filesystem for OneDriveFuse {
    fn open(&mut self, _req: &fuser::Request, ino: u64, flags: i32, reply: fuser::ReplyOpen) {
        debug!("OPEN: ino={}", ino);
        let item = self.get_item_by_ino(ino);
        let file_path = self.file_operations().file_exists_locally(item.virtual_ino().unwrap_or(0));
        if file_path.is_none() {
            reply.error(libc::ENOENT);
            return;
        }
        let file_path = file_path.unwrap();
        let open_flags = OpenFlags::from_i32(flags).unwrap();
        let mut open_options = OpenOptions::new();
        open_flags.apply_to(&mut open_options);
        
        match open_options.open(&file_path) {
            Ok(backend_file) => {
                // SUCCESS: We can create a stateful session.
                let boxed_file = Box::new(backend_file);
                let fh: u64 = Box::into_raw(boxed_file) as usize as u64;
                debug!("OPENED: fh={}", fh);
                reply.opened(fh, 0); // Return the valid FH
            },
            Err(e) => {
                // FAILURE: We cannot open the file (e.g., permission denied).
                // We must signal this. The kernel will then likely use
                // the direct path (fh=0) for subsequent operations, which will also fail.
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
                            .file_operations()
                            .file_exists_locally(item.virtual_ino().unwrap_or(0))
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
        let file_ptr = fh as usize as *mut std::fs::File;
        let _boxed_file: Box<std::fs::File> = unsafe { Box::from_raw(file_ptr) };
    
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
                &Duration::from_secs(3),
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
        let dots_added = self.add_dot_entries_if_needed(ino, &mut reply, offset);
        let current_offset = offset;
        let entries_added = if dots_added { 2 } else { 0 };
        let batch_size = 100; // Fetch 100 items at a time
                              // for offset 0 actual offset woudl be 0
                              // but if we have not added entries so offset was lets say 2 actual offset woudl be offset - 2
        let mut actual_offset: usize = if dots_added {
            offset as usize
        } else {
            (offset - 2) as usize
        }; //actual offset is the offset of the first child

        loop {
            let children = match sync_await(self.database().get_children_by_parent_ino_paginated(
                ino,
                actual_offset,
                batch_size,
            )) {
                Ok(children) => children,
                Err(_) => {
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
                    .file_operations()
                    .file_exists_locally(child.virtual_ino().unwrap_or(0))
                    .is_none()
                    && !child.is_folder()
                {
                    format!("{}.onedrivedownload", child.name().unwrap_or("unknown"))
                } else {
                    child.name().unwrap_or("unknown").to_string()
                };

                let entry_offset = if dots_added {
                    offset as i64 + i as i64 + 3 // Add 2 for "." and ".."
                } else {
                    offset as i64 + i as i64 + 1
                };

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
                actual_offset += 1;
            }
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

        if fh == 0 {
            // handle directIO
        let item = sync_await(self.database().get_item_by_ino(ino)).unwrap().unwrap();
        if item.is_folder() {
            reply.error(libc::EIO);
            return;
        }
        let file_path = self.file_operations().file_exists_locally(item.virtual_ino().unwrap_or(0));
        if let Some(file_path) = file_path {
            let mut file = OpenOptions::new().read(true).open(&file_path).unwrap();
            file.seek(SeekFrom::Start(offset as u64)).unwrap();
            let mut buffer = vec![0; size as usize];
            file.read_exact(&mut buffer).unwrap();
            reply.data(&buffer);
        }
            return;
        }

        // Check if this is a virtual file handle
        if fh == 1 {
            // VIRTUAL_FILE_HANDLE_ID
            if let Ok(Some(item)) = sync_await(self.database().get_item_by_ino(ino)) {
                if !item.is_folder() {
                    let content = self.file_operations().generate_placeholder_content(&item);

                    // Handle offset and size properly
                    let content_len = content.len() as i64;
                    let start = offset.min(content_len);
                    let end = (offset + size as i64).min(content_len);

                    if start < end {
                        let slice = &content[start as usize..end as usize];
                        reply.data(slice);
                        return;
                    } else {
                        reply.data(&[]); // Empty response for out-of-bounds reads
                    }
                    return;
                }
            }
            // If it's a folder or item not found, fall through to normal error handling
        }
        self.read_with_handle(ino, offset, size, reply);
        
        
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
            reply.error(libc::EIO);
            return;
        }
        if fh != 0 {
            self.write_with_handle(fh, offset, data, reply);
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
        let parent_item = self.get_item_by_ino(parent);
        let existing_driveItem = sync_await( self.drive_item_with_fuse_repo().get_drive_item_with_fuse_by_parent_ino_and_name_case_insensitive(parent, &name_str)).unwrap();
        let open_flags =  OpenFlags::from_i32(flags).unwrap();
        if open_flags.create_new && existing_driveItem.is_some() {
            reply.error(libc::EEXIST);
            return;
        }
        let mut open_options = OpenOptions::new();
        open_flags.apply_to(&mut open_options);
        if !open_flags.write && !open_flags.append {
            // If neither write nor append specified, default to write for creation
            open_options.write(true);
        }
        //We need to store Item now to obtain ID to know the path
        let new_item = sync_await(
            self.database()
                .apply_local_change_to_db_repository("create", parent, &name_str, false),
        ).unwrap();
        let new_file_path = self.file_manager().get_local_dir().join(new_item.to_string());

        match open_options.open(&new_file_path) {
            Ok(mut backend_file) => {
                
    
                // 9. Handle O_TRUNC - if file existed and we're truncating
                if open_flags.truncate && new_file_path.exists() {
                    if let Err(e) = backend_file.set_len(0) {
                        eprintln!("Warning: failed to truncate file: {}", e);
                    }
                }
    
                // 10. For O_APPEND, seek to end
                let mut seek_pos = 0;
                if open_flags.append {
                    if let Ok(metadata) = std::fs::metadata(&new_file_path) {
                        seek_pos = metadata.len();
                        if let Err(e) = backend_file.seek(SeekFrom::Start(seek_pos)) {
                            eprintln!("Warning: failed to seek to end: {}", e);
                        }
                    }
                }
    
                
    
                let metadata = match std::fs::metadata(&new_file_path) {
                    Ok(meta) => meta,
                    Err(e) => {
                        reply.error(e.raw_os_error().unwrap_or(libc::EIO));
                        return;
                    }
                };
    
                let attr = metadata.try_to_file_attr(new_item).unwrap();
    
            
    
                //  Create file handle
                let boxed_file = Box::new(backend_file);
                let fh = Box::into_raw(boxed_file) as usize as u64;
    
                //  Reply with created file info
                reply.created(&Duration::from_secs(1), &attr, 0, fh, 0);
            },
            Err(e) => {
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
                    let result = sync_await(
                        self.file_handles().create_processing_item_for_handle(&item.drive_item().id.clone())
                    );
                    if let Err(e) = result {
                        error!("Failed to create processing item for handle: {}", e);
                        reply.error(libc::EIO);
                        return;
                    }


                    reply.entry(
                        &Duration::from_secs(3),
                        &AttributeManager::item_to_file_attr(&item),
                        0,
                    );
                } else {
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
            }

            // Clean up any open handles for this inode
            if let Some(ino) = item.virtual_ino() {
                self.file_handles().cleanup_handles_for_inode(ino);
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
        let parent_item = self.get_item_by_ino(parent);
        let mut original_item = sync_await( self.drive_item_with_fuse_repo().get_drive_item_with_fuse_by_parent_ino_and_name_case_insensitive(parent, &name_str)).unwrap().unwrap();
        let existing_drive_item = sync_await( self.drive_item_with_fuse_repo().get_drive_item_with_fuse_by_parent_ino_and_name_case_insensitive(newparent, &newname_str)).unwrap();
        if existing_drive_item.is_some() {
            //its a replace! Special CASE!
                
        if flags & libc::RENAME_NOREPLACE != 0  {
            reply.error(libc::EEXIST);
            return;
        }

           let local_path = self.file_manager().get_local_dir();
           let local_path_from = local_path.join(original_item.virtual_ino().unwrap().to_string());
           let local_path_to = local_path.join(existing_drive_item.unwrap().virtual_ino().unwrap().to_string());
           if !local_path_from.exists() || !local_path_to.exists() {
            reply.error(libc::EEXIST);
            return;
           }
           std::fs::rename(&local_path_from, &local_path_to).unwrap();
           let _result = sync_await(self.drive_item_with_fuse_repo().delete_drive_item_with_fuse(original_item.drive_item().id.clone().as_str()));
           let delete_processing_item = crate::persistency::processing_item_repository::ProcessingItem::new_local(
            original_item.drive_item().clone(),  // Your DriveItem instance
            crate::sync::ChangeOperation::Delete
           );
           let processing_repo = self.app_state()
           .persistency()
           .processing_item_repository();
           let _result = sync_await(processing_repo.store_processing_item(&delete_processing_item));
           
           
           
            
           
            return;
        }
        
        let new_parent = self.get_item_by_ino(newparent);

        // Get the item to be renamed (case-insensitive lookup)
        if let Ok(Some(item)) = sync_await(
            self.drive_item_with_fuse_repo()
                .get_drive_item_with_fuse_by_parent_ino_and_name_case_insensitive(
                    parent, &name_str,
                ),
        ) {
            let mut updated_item = item.clone();

            // Update the name
            updated_item
                .drive_item_mut()
                .set_name(newname_str.to_string());

            // Update parent reference if moving to different parent
            if parent != newparent {
                if let Ok(Some(new_parent_item)) =
                    sync_await(self.database().get_item_by_ino(newparent))
                {
                    let new_parent_ref =
                        crate::onedrive_service::onedrive_models::ParentReference {
                            id: new_parent_item.id().to_string(),
                            path: new_parent_item
                                .virtual_path()
                                .map(|p| format!("/drive/root:{}", p)),
                        };
                    updated_item
                        .drive_item_mut()
                        .set_parent_reference(new_parent_ref);
                    updated_item.set_parent_ino(newparent);
                }
            }

            // Mark as local change
            updated_item.set_file_source(crate::persistency::types::FileSource::Local);

            // Store the updated item
            if let Err(e) = sync_await(
                self.drive_item_with_fuse_repo()
                    .store_drive_item_with_fuse(&updated_item),
            ) {
                error!("Failed to rename item: {}", e);
                reply.error(libc::EIO);
                return;
            }

            debug!(
                "📂 Renamed: {} -> {} ({})",
                name_str,
                newname_str,
                item.id()
            );
            reply.ok();
        } else {
            reply.error(libc::ENOENT);
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
            let path = self.file_operations().file_exists_locally(item.virtual_ino().unwrap_or(0));
            if path.is_some() {
                let file_path = path.unwrap();
                if let Some(new_size) = size {
                    match std::fs::OpenOptions::new().write(true).open(&file_path) {
                        Ok(mut file) => {
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
        if let Ok(children) = sync_await(self.database().get_children_by_parent_ino(ino)) {
            let mut entries = Vec::new();

            // Get the current item
            let item = match sync_await(self.database().get_item_by_ino(ino)) {
                Ok(Some(item)) => item,
                Ok(None) => {
                    reply.error(libc::ENOENT);
                    return;
                }
                Err(_) => {
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
                    .file_operations()
                    .file_exists_locally(child.virtual_ino().unwrap_or(0))
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
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn init(&mut self, _req: &fuser::Request<'_>, config: &mut KernelConfig) -> Result<(), c_int> {
        config
            .add_capabilities(FUSE_CAP_READDIRPLUS)
            .expect("Failed to add capabilities");
        Ok(())
    }
}
