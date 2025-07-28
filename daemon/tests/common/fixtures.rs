use onedrive_sync_daemon::onedrive_service::onedrive_models::{
    DriveItem, FileFacet, FolderFacet, ParentReference
};
use onedrive_sync_daemon::persistency::processing_item_repository::{
    ProcessingItem, ProcessingStatus, ChangeType, ChangeOperation
};
use chrono::Utc;

/// Create a test file DriveItem
pub fn create_test_file_item(id: &str, name: &str, parent_id: Option<String>) -> DriveItem {
    DriveItem {
        id: id.to_string(),
        name: Some(name.to_string()),
        etag: Some(format!("etag_{}", id)),
        last_modified: Some(Utc::now().to_rfc3339()),
        created_date: Some(Utc::now().to_rfc3339()),
        size: Some(1024),
        folder: None,
        file: Some(FileFacet {
            mime_type: Some("text/plain".to_string()),
        }),
        download_url: Some(format!("https://example.com/download/{}", id)),
        deleted: None,
        parent_reference: parent_id.map(|pid| ParentReference {
            id: pid,
            path: Some("/root".to_string()),
        }),
    }
}

/// Create a test folder DriveItem
pub fn create_test_folder_item(id: &str, name: &str, parent_id: Option<String>) -> DriveItem {
    DriveItem {
        id: id.to_string(),
        name: Some(name.to_string()),
        etag: Some(format!("etag_{}", id)),
        last_modified: Some(Utc::now().to_rfc3339()),
        created_date: Some(Utc::now().to_rfc3339()),
        size: None,
        folder: Some(FolderFacet { child_count: 0 }),
        file: None,
        download_url: None,
        deleted: None,
        parent_reference: parent_id.map(|pid| ParentReference {
            id: pid,
            path: Some("/root".to_string()),
        }),
    }
}

/// Create a test ProcessingItem for a file
pub fn create_test_processing_item(drive_item: DriveItem) -> ProcessingItem {
    ProcessingItem::new(drive_item)
}

/// Create a test ProcessingItem with specific status
pub fn create_test_processing_item_with_status(
    drive_item: DriveItem,
    status: ProcessingStatus,
) -> ProcessingItem {
    let mut item = ProcessingItem::new(drive_item);
    item.status = status;
    item
}

/// Create a test ProcessingItem for local changes
pub fn create_test_local_processing_item(
    drive_item: DriveItem,
    operation: ChangeOperation,
) -> ProcessingItem {
    ProcessingItem::new_local(drive_item, operation)
}

/// Create a test ProcessingItem for remote changes
pub fn create_test_remote_processing_item(
    drive_item: DriveItem,
    operation: ChangeOperation,
) -> ProcessingItem {
    ProcessingItem::new_remote(drive_item, operation)
}