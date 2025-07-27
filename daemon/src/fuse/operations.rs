//! FUSE filesystem operations implementation

use crate::fuse::filesystem::OneDriveFuse;
use crate::fuse::utils::{sync_await, FUSE_CAP_READDIRPLUS};
use crate::fuse::attributes::AttributeManager;
use crate::fuse::drive_item_manager::DriveItemManager;
use fuser::{
    FileAttr, FileType, KernelConfig, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory, ReplyEntry, ReplyStatfs, ReplyWrite
};
use libc::c_int;
use log::{debug, info, warn, error};
use std::ffi::OsStr;
use std::time::{Duration, SystemTime};

impl fuser::Filesystem for OneDriveFuse {
    fn open(&mut self, _req: &fuser::Request, ino: u64, _flags: i32, reply: fuser::ReplyOpen) {
        debug!("OPEN: ino={}", ino);
        
        if let Ok(Some(item)) = sync_await(self.database().get_item_by_ino(ino)) {
            if item.is_folder() {
                reply.opened(0, 0); // Directory - no file handle needed
                return;
            }
            
            // For files, try to open from local folder
            match self.file_handles().get_or_create_file_handle(ino, item.id()) {
                Ok(handle_id) => {
                    debug!("📂 Opened file handle {} for inode {} ({})", 
                           handle_id, ino, item.name().unwrap_or("unknown"));
                    reply.opened(handle_id, 0);
                }
                Err(e) => {
                    debug!("📂 File not found in local folder for inode but the Inode is real (downloadfile){}: {}", ino, e);
                    reply.opened(0,0);
                }
            }
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn release(&mut self, _req: &fuser::Request, ino: u64, fh: u64, _flags: i32, _lock_owner: Option<u64>, _flush: bool, reply: fuser::ReplyEmpty) {
        debug!("RELEASE: fh={}", fh);
        
        if fh == 0 {
            // Directory - nothing to close
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
                        if let Some(file_path) = self.file_operations().file_exists_locally(&item.drive_item().id) {
                            // Update DriveItem with file metadata
                            let mut updated_drive_item = item.drive_item().clone();
                            match sync_await(DriveItemManager::update_drive_item_from_file(&mut updated_drive_item, &file_path)) {
                                Ok(_) => {
                                    // Store the updated item
                                    let mut updated_item = item.clone();
                                    *updated_item.drive_item_mut() = updated_drive_item;
                                    match sync_await(self.drive_item_with_fuse_repo().store_drive_item_with_fuse(&updated_item)) {
                                        Ok(_) => {
                                            debug!("📂 Updated DriveItem metadata from file: {}", file_path.display());
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
        let close_result = self.file_handles().close_file_handle(fh);

        // Determine the final response
        match (update_result, close_result) {
            (Ok(_), Ok(_)) => {
                debug!("📂 Released file handle {}", fh);
                reply.ok();
            }
            (Ok(_), Err(e)) => {
                error!("Failed to close file handle {}: {}", fh, e);
                reply.error(libc::EIO);
            }
            (Err(_), Ok(_)) => {
                // Update failed but close succeeded
                reply.error(libc::EIO);
            }
            (Err(_), Err(e)) => {
                // Both update and close failed
                error!("Failed to close file handle {}: {}", fh, e);
                reply.error(libc::EIO);
            }
        }
    }

    fn lookup(&mut self, _req: &fuser::Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let name_str = name.to_string_lossy();
        info!("LOOKUP: parent={}, name={}", parent, name_str);

    

        // Strip .onedrivedownload extension if present for lookup
        let lookup_name = if name_str.ends_with(".onedrivedownload") {
            &name_str[..name_str.len() - 17] // Remove ".onedrivedownload"
        } else {
            &name_str
        };

        // Use optimized DB query by parent_ino and name
        if let Ok(Some(item)) = sync_await(self.drive_item_with_fuse_repo().get_drive_item_with_fuse_by_parent_ino_and_name(parent, lookup_name)) {
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
            reply.attr(&Duration::from_secs(3), &AttributeManager::item_to_file_attr(&item));
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
            if offset == 0 {
                let dot_ino = item.virtual_ino().unwrap_or(ino);
                let dotdot_ino = item.parent_ino().unwrap_or(1);
                entries.push((dot_ino, fuser::FileType::Directory, ".".to_string()));
                entries.push((dotdot_ino, fuser::FileType::Directory, "..".to_string()));
            }
            
            // Add child entries
            for (i, child) in children.iter().enumerate() {
                let entry_offset = (i + 2) as i64; // +2 for "." and ".."
                if entry_offset >= offset {
                    let file_type = if child.is_folder() {
                        fuser::FileType::Directory
                    } else {
                        fuser::FileType::RegularFile
                    };
                    let name = if self.file_operations().file_exists_locally(&child.drive_item().id).is_none() && !child.is_folder(){
                        format!("{}.onedrivedownload", child.name().unwrap_or("unknown"))
                    } else {
                        child.name().unwrap_or("unknown").to_string()
                    };
                    entries.push((child.virtual_ino().unwrap_or(0), file_type, name));
                }
            }

            let mut current_offset = offset+1 ;
            // Add entries with proper offset handling
            for (i, (ino, kind, name)) in entries.iter().enumerate() {
                

                
                
                if !reply.add(*ino, current_offset, *kind, name) {
                    break;
                }
                current_offset += 1;
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
        fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        debug!("READ: ino={}, fh={}, offset={}, size={}", ino, fh, offset, size);

        if fh == 0 {
            // Directory read - return empty data
            reply.data(&[]);
            return;
        }

        match self.file_handles().read_from_handle(fh, offset as u64, size) {
            Ok(data) => {
                reply.data(&data);
            }
            Err(e) => {
                error!("Failed to read from handle {}: {}", fh, e);
                reply.error(libc::EIO);
            }
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
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        debug!("WRITE: ino={}, fh={}, offset={}, size={}", ino, fh, offset, data.len());

        match self.file_handles().write_to_handle(fh, offset as u64, data) {
            Ok(_) => {
                reply.written(data.len() as u32);
            }
            Err(e) => {
                error!("Failed to write to handle {}: {}", fh, e);
                reply.error(libc::EIO);
            }
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

        match sync_await(self.database().apply_local_change_to_db_repository("create", parent, &name_str, false)) {
            Ok(ino) => {
                if let Ok(Some(item)) = sync_await(self.database().get_item_by_ino(ino)) {
                    // Create the actual file in the local directory
                    if let Err(e) = sync_await(self.file_manager().create_empty_file(item.id())) {
                        error!("Failed to create local file: {}", e);
                        reply.error(libc::EIO);
                        return;
                    }
                    
                    // Try to open the file
                    match self.file_handles().get_or_create_file_handle(ino, item.id()) {
                        Ok(handle_id) => {
                            reply.created(
                                &Duration::from_secs(3),
                                &AttributeManager::item_to_file_attr(&item),
                                handle_id,
                                0,
                                0,
                            );
                        }
                        Err(e) => {
                            error!("Failed to open created file: {}", e);
                            reply.error(libc::EIO);
                        }
                    }
                } else {
                    reply.error(libc::EIO);
                }
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

        match sync_await(self.database().apply_local_change_to_db_repository("mkdir", parent, &name_str, true)) {
            Ok(ino) => {
                if let Ok(Some(item)) = sync_await(self.database().get_item_by_ino(ino)) {
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

        // Get the item to be deleted
        if let Ok(Some(item)) = sync_await(self.drive_item_with_fuse_repo().get_drive_item_with_fuse_by_parent_ino_and_name(parent, &name_str)) {
            let onedrive_id = item.id();
            
            // Clean up any open handles for this inode
            if let Some(ino) = item.virtual_ino() {
                self.file_handles().cleanup_handles_for_inode(ino);
            }
            
            // Mark as deleted in database
            let mut updated_item = item.clone();
            updated_item.drive_item_mut().mark_deleted();
            
            if let Err(e) = sync_await(self.drive_item_with_fuse_repo().store_drive_item_with_fuse(&updated_item)) {
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

        // Get the directory to be deleted
        if let Ok(Some(item)) = sync_await(self.drive_item_with_fuse_repo().get_drive_item_with_fuse_by_parent_ino_and_name(parent, &name_str)) {
            let onedrive_id = item.id();
            
            // Check if directory is empty
            if let Ok(children) = sync_await(self.database().get_children_by_parent_ino(item.virtual_ino().unwrap_or(0))) {
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
            
            if let Err(e) = sync_await(self.drive_item_with_fuse_repo().store_drive_item_with_fuse(&updated_item)) {
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
        _flags: u32,
        reply: fuser::ReplyEmpty,
    ) {
        let name_str = name.to_string_lossy();
        let newname_str = newname.to_string_lossy();
        debug!("RENAME: parent={}, name={} -> newparent={}, newname={}", parent, name_str, newparent, newname_str);

        // Get the item to be renamed
        if let Ok(Some(item)) = sync_await(self.drive_item_with_fuse_repo().get_drive_item_with_fuse_by_parent_ino_and_name(parent, &name_str)) {
            let mut updated_item = item.clone();
            
            // Update the name
            updated_item.drive_item_mut().set_name(newname_str.to_string());
            
            // Update parent reference if moving to different parent
            if parent != newparent {
                if let Ok(Some(new_parent_item)) = sync_await(self.database().get_item_by_ino(newparent)) {
                    let new_parent_ref = crate::onedrive_service::onedrive_models::ParentReference {
                        id: new_parent_item.id().to_string(),
                        path: new_parent_item.virtual_path().map(|p| format!("/drive/root:{}", p)),
                    };
                    updated_item.drive_item_mut().set_parent_reference(new_parent_ref);
                    updated_item.set_parent_ino(newparent);
                }
            }
            
            // Mark as local change
            updated_item.set_file_source(crate::persistency::types::FileSource::Local);
            
            // Store the updated item
            if let Err(e) = sync_await(self.drive_item_with_fuse_repo().store_drive_item_with_fuse(&updated_item)) {
                error!("Failed to rename item: {}", e);
                reply.error(libc::EIO);
                return;
            }
            
            debug!("📂 Renamed: {} -> {} ({})", name_str, newname_str, item.id());
            reply.ok();
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

        if let Ok(Some(item)) = sync_await(self.database().get_item_by_ino(ino)) {
            // Mark as modified if any attributes changed
            if size.is_some() || mtime.is_some() {
                if let Err(e) = sync_await(self.database().mark_db_item_as_modified(ino)) {
                    warn!("Failed to mark item as modified: {}", e);
                }
            }

            reply.attr(&Duration::from_secs(1), &AttributeManager::item_to_file_attr(&item));
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


} 