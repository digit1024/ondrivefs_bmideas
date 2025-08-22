//! DriveItem management and update functionality

use crate::onedrive_service::onedrive_models::DriveItem;
use crate::persistency::types::{DriveItemWithFuse, FileSource};
use anyhow::Result;
use log::debug;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// DriveItem manager for the FUSE filesystem
pub struct DriveItemManager;

impl DriveItemManager {
    /// Update DriveItem data from existing file metadata
    /// This helper function extracts file metadata and updates the DriveItem accordingly
    pub async fn update_drive_item_from_file(
        drive_item: &mut DriveItem,
        file_path: &Path,
    ) -> Result<()> {
        use chrono::{DateTime, Utc};
        use std::fs;

        // Get file metadata
        let metadata = fs::metadata(file_path)?;

        // Update size
        drive_item.set_size(metadata.size());

        // Update last modified time
        if let Ok(modified_time) = metadata.modified() {
            let datetime: DateTime<Utc> = modified_time.into();
            drive_item.set_last_modified(datetime.format("%Y-%m-%dT%H:%M:%SZ").to_string());
        }

        // Update created time if not already set
        if drive_item.created_date.is_none() {
            if let Ok(created_time) = metadata.created() {
                let datetime: DateTime<Utc> = created_time.into();
                drive_item.set_created_date(datetime.format("%Y-%m-%dT%H:%M:%SZ").to_string());
            }
        }

        // Update file facet with MIME type if it's a file
        if metadata.is_file() {
            let mime_type = Self::guess_mime_type(file_path);
            let file_facet = crate::onedrive_service::onedrive_models::FileFacet { mime_type };
            drive_item.set_file(file_facet);
        } else if metadata.is_dir() {
            // Set folder facet if it's a directory
            let folder_facet = crate::onedrive_service::onedrive_models::FolderFacet {
                child_count: 0, // This would need to be calculated separately
            };
            drive_item.set_folder(folder_facet);
        }

        debug!(
            "📂 Updated DriveItem from file metadata: {} (size: {})",
            file_path.display(),
            metadata.size()
        );

        Ok(())
    }

    /// Guess MIME type based on file extension
    pub fn guess_mime_type(file_path: &Path) -> Option<String> {
        // Simple MIME type mapping
        let mime_types = [
            ("txt", "text/plain"),
            ("html", "text/html"),
            ("htm", "text/html"),
            ("css", "text/css"),
            ("js", "application/javascript"),
            ("json", "application/json"),
            ("xml", "application/xml"),
            ("pdf", "application/pdf"),
            ("zip", "application/zip"),
            ("tar", "application/x-tar"),
            ("gz", "application/gzip"),
            ("jpg", "image/jpeg"),
            ("jpeg", "image/jpeg"),
            ("png", "image/png"),
            ("gif", "image/gif"),
            ("svg", "image/svg+xml"),
            ("mp3", "audio/mpeg"),
            ("mp4", "video/mp4"),
            ("avi", "video/x-msvideo"),
            ("doc", "application/msword"),
            (
                "docx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ),
            ("xls", "application/vnd.ms-excel"),
            (
                "xlsx",
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            ),
            ("ppt", "application/vnd.ms-powerpoint"),
            (
                "pptx",
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            ),
        ];

        file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .and_then(|ext| {
                mime_types
                    .iter()
                    .find(|(extension, _)| *extension == ext)
                    .map(|(_, mime_type)| *mime_type)
            })
            .map(|mime_type| mime_type.to_string())
    }

    /// Create a temporary root stub for FUSE operations (not stored in DB)
    pub fn create_temp_root_stub(
        drive_item_with_fuse_repo: &crate::persistency::drive_item_with_fuse_repository::DriveItemWithFuseRepository,
    ) -> DriveItemWithFuse {
        let root_drive_item = DriveItem {
            id: "temp_root".to_string(),
            name: Some("root".to_string()),
            etag: None,
            ctag: None,
            last_modified: None,
            created_date: None,
            size: Some(0),
            folder: Some(crate::onedrive_service::onedrive_models::FolderFacet { child_count: 0 }),
            file: None,
            download_url: None,
            deleted: None,
            parent_reference: None,
        };

        let mut root_with_fuse = drive_item_with_fuse_repo.create_from_drive_item(root_drive_item);
        root_with_fuse.set_virtual_ino(1);
        root_with_fuse.set_virtual_path("/".to_string());

        root_with_fuse.set_file_source(FileSource::Local); // Mark as local since it's a stub
        root_with_fuse.set_sync_status("stub".to_string());

        root_with_fuse
    }
}
