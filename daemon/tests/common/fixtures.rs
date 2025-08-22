#![allow(dead_code)]

use chrono::Utc;
use onedrive_sync_daemon::onedrive_service::onedrive_models::{
    DriveItem, FileFacet, FolderFacet, ParentReference,
};
use onedrive_sync_daemon::persistency::processing_item_repository::{
    ChangeOperation, ChangeType, ProcessingItem, ProcessingStatus,
};
use onedrive_sync_daemon::persistency::types::{DriveItemWithFuse, FileSource, FuseMetadata};
use std::collections::HashMap;

/// Create a test file DriveItem
pub fn create_test_file_item(id: &str, name: &str, parent_id: Option<String>) -> DriveItem {
    DriveItem {
        id: id.to_string(),
        name: Some(name.to_string()),
        etag: Some(format!("etag_{}", id)),
        ctag: Some(format!("ctag_{}", id)),
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
        ctag: Some(format!("ctag_{}", id)),
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

/// Create a test DriveItemWithFuse for a file
pub fn create_test_drive_item_with_fuse_file(
    id: &str,
    name: &str,
    parent_id: Option<String>,
    virtual_ino: Option<u64>,
    parent_ino: Option<u64>,
) -> DriveItemWithFuse {
    let drive_item = create_test_file_item(id, name, parent_id);
    let mut item_with_fuse = DriveItemWithFuse::from_drive_item(drive_item);

    if let Some(ino) = virtual_ino {
        item_with_fuse.set_virtual_ino(ino);
    }

    if let Some(p_ino) = parent_ino {
        item_with_fuse.set_parent_ino(p_ino);
    }

    // Set virtual path
    let virtual_path = item_with_fuse.compute_virtual_path();
    item_with_fuse.set_virtual_path(virtual_path);

    item_with_fuse
}

/// Create a test DriveItemWithFuse for a folder
pub fn create_test_drive_item_with_fuse_folder(
    id: &str,
    name: &str,
    parent_id: Option<String>,
    virtual_ino: Option<u64>,
    parent_ino: Option<u64>,
) -> DriveItemWithFuse {
    let drive_item = create_test_folder_item(id, name, parent_id);
    let mut item_with_fuse = DriveItemWithFuse::from_drive_item(drive_item);

    if let Some(ino) = virtual_ino {
        item_with_fuse.set_virtual_ino(ino);
    }

    if let Some(p_ino) = parent_ino {
        item_with_fuse.set_parent_ino(p_ino);
    }

    // Set virtual path
    let virtual_path = item_with_fuse.compute_virtual_path();
    item_with_fuse.set_virtual_path(virtual_path);

    item_with_fuse
}

/// Create a test DriveItemWithFuse with custom file source
pub fn create_test_drive_item_with_fuse_custom(
    drive_item: DriveItem,
    virtual_ino: Option<u64>,
    parent_ino: Option<u64>,
    file_source: FileSource,
) -> DriveItemWithFuse {
    let mut item_with_fuse = DriveItemWithFuse::from_drive_item(drive_item);

    if let Some(ino) = virtual_ino {
        item_with_fuse.set_virtual_ino(ino);
    }

    if let Some(p_ino) = parent_ino {
        item_with_fuse.set_parent_ino(p_ino);
    }

    item_with_fuse.set_file_source(file_source);

    // Set virtual path
    let virtual_path = item_with_fuse.compute_virtual_path();
    item_with_fuse.set_virtual_path(virtual_path);

    item_with_fuse
}

/// Tree item structure for building hierarchical data
#[derive(Debug, Clone)]
enum TreeItem {
    File(String),
    Folder(String, Vec<TreeItem>),
}

/// Generate a tree of DriveItems with valid parent relations (around 50 elements)
/// Creates a realistic file structure with folders and files
pub fn create_drive_items_tree() -> Vec<DriveItemWithFuse> {
    let mut items = Vec::new();
    let mut inode_counter = 1u64;
    let mut id_counter = 1u64;

    // Helper function to generate unique IDs
    let mut generate_id = || {
        let id = format!("E867B1C99DE243C4!{}", id_counter);
        id_counter += 1;
        id
    };

    // Helper function to get next inode
    let mut next_inode = || {
        let ino = inode_counter;
        inode_counter += 1;
        ino
    };

    // Root folder
    let root_id = generate_id();
    let root_ino = next_inode();
    let root_item =
        create_test_drive_item_with_fuse_folder(&root_id, "root", None, Some(root_ino), None);
    items.push(root_item);

    // Define the tree structure
    let tree_structure = vec![
        TreeItem::Folder(
            "Documents".to_string(),
            vec![
                //2
                TreeItem::Folder(
                    "Work".to_string(),
                    vec![
                        //3
                        TreeItem::Folder(
                            "Reports".to_string(),
                            vec![
                                //4
                                TreeItem::File("Q1_Report.pdf".to_string()), // id = 5
                                TreeItem::File("Q2_Report.pdf".to_string()), // id = 6
                                TreeItem::File("Q3_Report.pdf".to_string()), // id = 7
                                TreeItem::File("Q4_Report.pdf".to_string()), // id = 8
                            ],
                        ),
                        TreeItem::Folder(
                            "Presentations".to_string(),
                            vec![
                                TreeItem::File("Team_Meeting.pptx".to_string()),
                                TreeItem::File("Client_Presentation.pptx".to_string()),
                            ],
                        ),
                        TreeItem::Folder(
                            "Contracts".to_string(),
                            vec![
                                TreeItem::File("Contract_2024.pdf".to_string()),
                                TreeItem::File("Agreement.docx".to_string()),
                            ],
                        ),
                    ],
                ),
                TreeItem::Folder(
                    "Personal".to_string(),
                    vec![
                        TreeItem::File("Resume.pdf".to_string()),
                        TreeItem::File("Cover_Letter.docx".to_string()),
                    ],
                ),
            ],
        ),
        TreeItem::Folder(
            "Pictures".to_string(),
            vec![
                TreeItem::Folder(
                    "Vacation".to_string(),
                    vec![
                        TreeItem::File("Beach_2024.jpg".to_string()),
                        TreeItem::File("Mountain_2024.jpg".to_string()),
                        TreeItem::File("City_2024.jpg".to_string()),
                    ],
                ),
                TreeItem::Folder(
                    "Family".to_string(),
                    vec![
                        TreeItem::File("Birthday.jpg".to_string()),
                        TreeItem::File("Christmas.jpg".to_string()),
                    ],
                ),
                TreeItem::Folder(
                    "Screenshots".to_string(),
                    vec![
                        TreeItem::File("Screenshot_001.png".to_string()),
                        TreeItem::File("Screenshot_002.png".to_string()),
                    ],
                ),
            ],
        ),
        TreeItem::Folder(
            "Music".to_string(),
            vec![
                TreeItem::Folder(
                    "Rock".to_string(),
                    vec![
                        TreeItem::File("Song1.mp3".to_string()),
                        TreeItem::File("Song2.mp3".to_string()),
                    ],
                ),
                TreeItem::Folder(
                    "Jazz".to_string(),
                    vec![
                        TreeItem::File("Jazz1.mp3".to_string()),
                        TreeItem::File("Jazz2.mp3".to_string()),
                    ],
                ),
            ],
        ),
        TreeItem::Folder(
            "Videos".to_string(),
            vec![
                TreeItem::Folder(
                    "Movies".to_string(),
                    vec![
                        TreeItem::File("Movie1.mp4".to_string()),
                        TreeItem::File("Movie2.mp4".to_string()),
                    ],
                ),
                TreeItem::Folder(
                    "Tutorials".to_string(),
                    vec![
                        TreeItem::File("Tutorial1.mp4".to_string()),
                        TreeItem::File("Tutorial2.mp4".to_string()),
                    ],
                ),
            ],
        ),
        TreeItem::Folder(
            "Downloads".to_string(),
            vec![
                TreeItem::Folder(
                    "Software".to_string(),
                    vec![
                        TreeItem::File("app1.exe".to_string()),
                        TreeItem::File("app2.dmg".to_string()),
                    ],
                ),
                TreeItem::Folder(
                    "Temp".to_string(),
                    vec![
                        TreeItem::File("temp1.txt".to_string()),
                        TreeItem::File("temp2.txt".to_string()),
                    ],
                ),
            ],
        ),
    ];

    // Build the tree structure
    for tree_item in tree_structure {
        process_tree_item(
            &mut items,
            &mut generate_id,
            &mut next_inode,
            &root_id,
            root_ino,
            tree_item,
        );
    }

    items
}

/// Recursive helper function to process tree items
fn process_tree_item(
    items: &mut Vec<DriveItemWithFuse>,
    generate_id: &mut impl FnMut() -> String,
    next_inode: &mut impl FnMut() -> u64,
    parent_id: &str,
    parent_ino: u64,
    tree_item: TreeItem,
) {
    match tree_item {
        TreeItem::File(name) => {
            let item_id = generate_id();
            let item_ino = next_inode();
            let file_item = create_test_drive_item_with_fuse_file(
                &item_id,
                &name,
                Some(parent_id.to_string()),
                Some(item_ino),
                Some(parent_ino),
            );
            items.push(file_item);
        }
        TreeItem::Folder(name, children) => {
            let item_id = generate_id();
            let item_ino = next_inode();
            let folder_item = create_test_drive_item_with_fuse_folder(
                &item_id,
                &name,
                Some(parent_id.to_string()),
                Some(item_ino),
                Some(parent_ino),
            );
            items.push(folder_item);

            // Process children recursively
            for child in children {
                process_tree_item(items, generate_id, next_inode, &item_id, item_ino, child);
            }
        }
    }
}

/// Create a simple flat structure of DriveItems (useful for testing)
pub fn create_flat_drive_items_structure(count: usize) -> Vec<DriveItemWithFuse> {
    let mut items = Vec::new();
    let mut inode_counter = 1u64;
    let mut id_counter = 1u64;

    // Root folder
    let root_id = format!("E867B1C99DE243C4!{}", id_counter);
    id_counter += 1;
    let root_ino = inode_counter;
    inode_counter += 1;

    let root_item =
        create_test_drive_item_with_fuse_folder(&root_id, "root", None, Some(root_ino), None);
    items.push(root_item);

    // Create flat structure of files and folders
    for i in 0..count {
        let is_folder = i % 3 == 0; // Every 3rd item is a folder
        let item_id = format!("E867B1C99DE243C4!{}", id_counter);
        id_counter += 1;
        let item_ino = inode_counter;
        inode_counter += 1;

        let item_name = if is_folder {
            format!("folder_{}", i)
        } else {
            format!("file_{}.txt", i)
        };

        let item = if is_folder {
            create_test_drive_item_with_fuse_folder(
                &item_id,
                &item_name,
                Some(root_id.clone()),
                Some(item_ino),
                Some(root_ino),
            )
        } else {
            create_test_drive_item_with_fuse_file(
                &item_id,
                &item_name,
                Some(root_id.clone()),
                Some(item_ino),
                Some(root_ino),
            )
        };

        items.push(item);
    }

    items
}
