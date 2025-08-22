#![allow(dead_code)]

use crate::onedrive_service::onedrive_models::DriveItem;
use std::path::PathBuf;

/// Download queue item for tracking pending downloads
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DownloadQueueItem {
    pub id: i64,
    pub onedrive_id: String,
    pub local_path: PathBuf,
    pub priority: i32,
    pub status: String,
    pub retry_count: i32,
    pub created_at: String,
    pub updated_at: String,
    pub ino: u64,
    pub name: String,
    pub virtual_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum FileSource {
    Remote, // From OneDrive (DriveItems)
    Local,  // From local changes (LocalChanges)
    Merged, // Merged from both sources
}

impl FileSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileSource::Remote => "remote",
            FileSource::Local => "local",
            FileSource::Merged => "merged",
        }
    }
}

/// Fuse filesystem metadata
#[derive(Debug, Clone)]
pub struct FuseMetadata {
    pub virtual_ino: Option<u64>,
    pub parent_ino: Option<u64>,
    pub virtual_path: Option<String>,
    pub file_source: Option<FileSource>,
    pub sync_status: Option<String>,
    pub ctag: Option<String>,
}

/// Complete item with both OneDrive and Fuse data
#[derive(Debug, Clone)]
pub struct DriveItemWithFuse {
    pub drive_item: DriveItem,
    pub fuse_metadata: FuseMetadata,
}

impl DriveItemWithFuse {
    /// Create from OneDrive API response
    pub fn from_drive_item(drive_item: DriveItem) -> Self {
        Self {
            drive_item,
            fuse_metadata: FuseMetadata {
                virtual_ino: None,
                parent_ino: None,
                virtual_path: None,

                file_source: Some(FileSource::Remote),
                sync_status: None,
                ctag: None,
            },
        }
    }

    /// Get virtual path (computed from parent reference)
    pub fn compute_virtual_path(&self) -> String {
        if let Some(parent_ref) = &self.drive_item.parent_reference {
            if let Some(parent_path) = &parent_ref.path {
                let mut path = parent_path.replace("/drive/root:", "");
                if !path.starts_with('/') {
                    path = format!("/{}", path);
                }
                if path == "/" {
                    format!("/{}", self.drive_item.name.as_deref().unwrap_or(""))
                } else {
                    format!("{}/{}", path, self.drive_item.name.as_deref().unwrap_or(""))
                }
            } else {
                format!("/{}", self.drive_item.name.as_deref().unwrap_or(""))
            }
        } else {
            format!("/{}", self.drive_item.name.as_deref().unwrap_or(""))
        }
    }

    /// Update Fuse metadata
    pub fn update_fuse_metadata(&mut self, metadata: FuseMetadata) {
        self.fuse_metadata = metadata;
    }

    /// Set virtual inode
    pub fn set_virtual_ino(&mut self, ino: u64) {
        self.fuse_metadata.virtual_ino = Some(ino);
    }

    /// Set parent inode
    pub fn set_parent_ino(&mut self, parent_ino: u64) {
        self.fuse_metadata.parent_ino = Some(parent_ino);
    }

    /// Set file source
    pub fn set_file_source(&mut self, source: FileSource) {
        self.fuse_metadata.file_source = Some(source);
    }

    /// Set virtual path
    pub fn set_virtual_path(&mut self, path: String) {
        self.fuse_metadata.virtual_path = Some(path);
    }

    /// Set sync status
    pub fn set_sync_status(&mut self, status: String) {
        self.fuse_metadata.sync_status = Some(status);
    }

    /// Set ctag
    pub fn set_ctag(&mut self, ctag: String) {
        self.fuse_metadata.ctag = Some(ctag);
    }

    /// Get virtual inode
    pub fn virtual_ino(&self) -> Option<u64> {
        self.fuse_metadata.virtual_ino
    }

    /// Get parent inode
    pub fn parent_ino(&self) -> Option<u64> {
        self.fuse_metadata.parent_ino
    }

    /// Get virtual path
    pub fn virtual_path(&self) -> Option<&str> {
        self.fuse_metadata.virtual_path.as_deref()
    }

    /// Get file source
    pub fn file_source(&self) -> Option<FileSource> {
        self.fuse_metadata.file_source
    }

    /// Get sync status
    pub fn sync_status(&self) -> Option<&str> {
        self.fuse_metadata.sync_status.as_deref()
    }

    /// Get ctag
    pub fn ctag(&self) -> Option<&str> {
        self.fuse_metadata.ctag.as_deref()
    }

    /// Get Fuse metadata
    pub fn fuse_metadata(&self) -> &FuseMetadata {
        &self.fuse_metadata
    }

    /// Get mutable Fuse metadata
    pub fn fuse_metadata_mut(&mut self) -> &mut FuseMetadata {
        &mut self.fuse_metadata
    }
    #[allow(dead_code)]
    pub(crate) fn set_parent_id(&mut self, id: String) {
        self.drive_item_mut().parent_reference =
            Some(crate::onedrive_service::onedrive_models::ParentReference { id: id, path: None });
    }
    #[allow(dead_code)]
    pub fn is_synced(&self) -> bool {
        !self.drive_item.id.starts_with("local_")
    }
}

// Delegate common accessors to DriveItem
#[allow(dead_code)]
impl DriveItemWithFuse {
    pub fn id(&self) -> &str {
        &self.drive_item.id
    }

    pub fn name(&self) -> Option<&str> {
        self.drive_item.name.as_deref()
    }

    pub fn is_folder(&self) -> bool {
        self.drive_item.folder.is_some()
    }

    pub fn size(&self) -> u64 {
        self.drive_item.size.unwrap_or(0) as u64
    }

    pub fn etag(&self) -> Option<&str> {
        self.drive_item.etag.as_deref()
    }

    pub fn last_modified(&self) -> Option<&str> {
        self.drive_item.last_modified.as_deref()
    }

    pub fn created_date(&self) -> Option<&str> {
        self.drive_item.created_date.as_deref()
    }

    pub fn download_url(&self) -> Option<&str> {
        self.drive_item.download_url.as_deref()
    }

    pub fn mime_type(&self) -> Option<&str> {
        self.drive_item
            .file
            .as_ref()
            .and_then(|f| f.mime_type.as_deref())
    }

    pub fn parent_reference(
        &self,
    ) -> Option<&crate::onedrive_service::onedrive_models::ParentReference> {
        self.drive_item.parent_reference.as_ref()
    }

    pub fn is_deleted(&self) -> bool {
        self.drive_item.deleted.is_some()
    }

    /// Get the underlying DriveItem
    pub fn drive_item(&self) -> &DriveItem {
        &self.drive_item
    }

    /// Get mutable access to the underlying DriveItem
    pub fn drive_item_mut(&mut self) -> &mut DriveItem {
        &mut self.drive_item
    }
}
