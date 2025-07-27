//! File attribute conversion for FUSE filesystem

use crate::persistency::types::DriveItemWithFuse;
use fuser::{FileAttr, FileType};
use sqlx::types::chrono;
use std::time::{SystemTime, UNIX_EPOCH};

/// Attribute manager for the FUSE filesystem
pub struct AttributeManager;

impl AttributeManager {
    /// Convert DriveItemWithFuse to FUSE FileAttr
    pub fn item_to_file_attr(item: &DriveItemWithFuse) -> FileAttr {
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
    pub fn create_default_attr(ino: u64, is_folder: bool) -> FileAttr {
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
} 