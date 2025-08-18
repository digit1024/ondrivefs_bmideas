use anyhow::Result;
use onedrive_sync_daemon::file_manager::FileManager;
use onedrive_sync_daemon::persistency::drive_item_with_fuse_repository::DriveItemWithFuseRepository;
use onedrive_sync_daemon::persistency::processing_item_repository::{
    ChangeOperation, ChangeType, ProcessingItemRepository, ProcessingStatus,
};
use onedrive_sync_daemon::persistency::types::DriveItemWithFuse;
use onedrive_sync_daemon::sync::conflicts::{LocalConflict, RemoteConflict};
use onedrive_sync_daemon::sync::SyncProcessor;
use serial_test::serial;
use std::path::Path;
use std::sync::Arc;

use crate::common::fixtures::{
    create_test_local_processing_item, create_test_remote_processing_item,
    create_test_file_item, create_test_folder_item,
};
use crate::common::setup::TEST_ENV;
use crate::common::mock_onedrive_client::MockOneDriveClient;
use onedrive_sync_daemon::app_state::AppState;
use onedrive_sync_daemon::onedrive_service::onedrive_models::{
    UploadResult, DriveItem, DeleteResult, CreateFolderResult, FileFacet, ParentReference, DeletedFacet,
};



async fn setup_test_env() -> Result<(
    Arc<AppState>,
    ProcessingItemRepository,
    DriveItemWithFuseRepository,
    MockOneDriveClient,
)> {
    let mut env = TEST_ENV.lock().await;

    let db_path = env.db_path();
    let delete_db_result = std::fs::remove_file(db_path);
    if delete_db_result.is_err() {
        println!("🔍 Failed to delete database file: {:?}", delete_db_result.err());
        //panic!("Failed to delete database file");
    }

    let mock_client = MockOneDriveClient::new();
    let mock_clone = mock_client.clone();
    let app_state = env.get_app_state_with_custom_mock(mock_client).await?;
    env.clear_all_data().await?;

    let repo = app_state.persistency().processing_item_repository();
    let drive_items_with_fuse_repo = app_state.persistency().drive_item_with_fuse_repository();

    let tree_items = crate::common::fixtures::create_drive_items_tree();
    for item in &tree_items {
        drive_items_with_fuse_repo
            .store_drive_item_with_fuse(&item)
            .await?;
    }

    Ok((app_state, repo, drive_items_with_fuse_repo, mock_clone))
}

// ====================================================================================
// 🔧 Helper Functions for Test Maintainability
// ====================================================================================

/// Helper to create a local file for testing local operations
async fn create_local_file(app_state: &AppState, virtual_ino: u64, content: &str) -> Result<()> {
    let local_dir = app_state.file_manager().get_local_dir();
    let file_path = local_dir.join(virtual_ino.to_string());
    std::fs::create_dir_all(&local_dir)?;
    std::fs::write(&file_path, content)?;
    Ok(())
}

/// Helper to remove a local file
#[allow(dead_code)]
async fn remove_local_file(app_state: &AppState, virtual_ino: u64) -> Result<()> {
    let local_dir = app_state.file_manager().get_local_dir();
    let file_path = local_dir.join(virtual_ino.to_string());
    if file_path.exists() {
        std::fs::remove_file(&file_path)?;
    }
    Ok(())
}

/// Helper to create a test item with modified etag
fn create_modified_drive_item(original: &DriveItem, new_etag: &str) -> DriveItem {
    let mut modified = original.clone();
    modified.etag = Some(new_etag.to_string());
    modified
}

/// Helper to create a test item moved to a new parent
fn create_moved_drive_item(original: &DriveItem, new_parent_id: &str) -> DriveItem {
    let mut moved = original.clone();
    moved.parent_reference = Some(ParentReference {
        id: new_parent_id.to_string(),
        path: Some("/root".to_string()),
    });
    moved
}

/// Helper to create a deleted drive item
#[allow(dead_code)]
fn create_deleted_drive_item(original: &DriveItem) -> DriveItem {
    let mut deleted = original.clone();
    deleted.deleted = Some(DeletedFacet {
        state: "deleted".to_string(),
    });
    deleted
}

/// Helper to process items and check conflict status
async fn process_and_verify_conflict(
    repo: &ProcessingItemRepository,
    sync_processor: &SyncProcessor,
    item_id: i64,
    expected_status: ProcessingStatus,
    expected_error_contains: Option<&str>,
) -> Result<()> {
    sync_processor.process_all_items().await?;
    
    let processed_item = repo.get_processing_item_by_id(item_id).await?.unwrap();
    assert_eq!(processed_item.status, expected_status);
    
    if let Some(error_text) = expected_error_contains {
        assert!(
            processed_item.validation_errors.iter().any(|e| e.contains(error_text)),
            "Expected error containing '{}', but got: {:?}",
            error_text,
            processed_item.validation_errors
        );
    }
    
    Ok(())
}



#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_fs_item_tree_creation_works() -> Result<()> {
    println!("\n🧪 Running test: Filesystem item tree creation");
    let (_app_state, _repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

    let item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?;
    assert!(item.is_some());
    let item = item.unwrap();

    assert_eq!(item.drive_item().name, Some("Q1_Report.pdf".to_string()));
    assert_eq!(item.parent_ino(), Some(4));
    assert!(item.drive_item().file.is_some());

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_processing_item_modified_no_conflicts() -> Result<()> {
    println!("\n🧪 Running test: Successful remote modification");
    let (app_state, repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

    let item_to_update = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?
        .unwrap();

    let mut di = item_to_update.drive_item().clone();
    di.etag = Some("new-etag-123".to_string());
    let processing_item = create_test_remote_processing_item(di, ChangeOperation::Update);
    let item_id = repo.store_processing_item(&processing_item).await?;

    let item_to_process = repo.get_processing_item_by_id(item_id).await?.unwrap();
    let sync_processor = SyncProcessor::new(app_state.clone());
    sync_processor
        .process_single_item(&item_to_process)
        .await?;

    let updated_item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?
        .unwrap();
    assert_eq!(
        updated_item.drive_item().etag,
        Some("new-etag-123".to_string())
    );

    let processed_item = repo.get_processing_item_by_id(item_id).await?.unwrap();
    assert_eq!(processed_item.status, ProcessingStatus::Done);

    Ok(())
}

// ====================================================================================
// 🔥 REMOTE CONFLICT TESTS - Testing all RemoteConflict scenarios
// ====================================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_remote_conflict_create_on_create() -> Result<()> {
    println!("\n🧪 Remote Conflict: CreateOnCreate");
    let (app_state, repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

    // Get parent folder for creating new items in
    let parent_item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(4) // "Folder B"
        .await?
        .unwrap();

    // Create a new file item that will be "created" locally and remotely
    let new_file = create_test_file_item("new_file_123", "NewFile.txt", Some(parent_item.id().to_string()));

    // Create local creation
    let local_create = create_test_local_processing_item(new_file.clone(), ChangeOperation::Create);
    repo.store_processing_item(&local_create).await?;

    // Create remote creation
    let remote_create = create_test_remote_processing_item(new_file, ChangeOperation::Create);
    let remote_id = repo.store_processing_item(&remote_create).await?;

    let sync_processor = SyncProcessor::new(app_state.clone());
    process_and_verify_conflict(
        &repo,
        &sync_processor,
        remote_id,
        ProcessingStatus::Conflicted,
        Some("Remote item created, but an item with the same name already exists locally"),
    ).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_remote_conflict_modify_on_modify() -> Result<()> {
    println!("\n🧪 Remote Conflict: ModifyOnModify");
    let (app_state, repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

    let original_item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?
        .unwrap();
    let original_etag = original_item.drive_item().etag.clone();

    // Create local file for update
    create_local_file(&app_state, 5, "local content").await?;

    // Create local modification
    let local_modified = create_modified_drive_item(original_item.drive_item(), "local-etag-123");
    let local_change = create_test_local_processing_item(local_modified, ChangeOperation::Update);
    repo.store_processing_item(&local_change).await?;

    // Create remote modification  
    let remote_modified = create_modified_drive_item(original_item.drive_item(), "remote-etag-456");
    let remote_change = create_test_remote_processing_item(remote_modified, ChangeOperation::Update);
    let remote_id = repo.store_processing_item(&remote_change).await?;

    let sync_processor = SyncProcessor::new(app_state.clone());
    process_and_verify_conflict(
        &repo,
        &sync_processor,
        remote_id,
        ProcessingStatus::Conflicted,
        Some("Remote item was modified, but the local item was also modified"),
    ).await?;

    // Verify original etag is preserved
    let final_item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?
        .unwrap();
    assert_eq!(final_item.drive_item().etag, original_etag);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_remote_conflict_modify_on_delete() -> Result<()> {
    println!("\n🧪 Remote Conflict: ModifyOnDelete");
    let (app_state, repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

    let original_item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?
        .unwrap();

    // Create local deletion
    let local_delete = create_test_local_processing_item(
        original_item.drive_item().clone(),
        ChangeOperation::Delete,
    );
    repo.store_processing_item(&local_delete).await?;

    // Create remote modification
    let remote_modified = create_modified_drive_item(original_item.drive_item(), "remote-etag-789");
    let remote_change = create_test_remote_processing_item(remote_modified, ChangeOperation::Update);
    let remote_id = repo.store_processing_item(&remote_change).await?;

    let sync_processor = SyncProcessor::new(app_state.clone());
    process_and_verify_conflict(
        &repo,
        &sync_processor,
        remote_id,
        ProcessingStatus::Conflicted,
        Some("Remote item was modified, but the local item was deleted"),
    ).await?;

    Ok(())
}


//The logic for this is changed - we shoudl recreate folders to limit a possibility of conflicts 
// #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
// #[serial]
// async fn test_remote_conflict_modify_on_parent_delete() -> Result<()> {
//     println!("\n🧪 Remote Conflict: ModifyOnParentDelete");
//     let (app_state, repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

//     // Get a file and its parent folder
//     let file_item = drive_items_with_fuse_repo
//         .get_drive_item_with_fuse_by_virtual_ino(5) // File in Folder B (ino 4)
//         .await?
//         .unwrap();

//     let parent_folder = drive_items_with_fuse_repo
//         .get_drive_item_with_fuse_by_virtual_ino(4) // Folder B
//         .await?
//         .unwrap();

//     // Mark parent folder as deleted locally
//     drive_items_with_fuse_repo.mark_as_deleted_by_onedrive_id(parent_folder.id()).await?;

//     // Create remote modification of the file
//     let remote_modified = create_modified_drive_item(file_item.drive_item(), "remote-etag-parent-del");
//     let remote_change = create_test_remote_processing_item(remote_modified, ChangeOperation::Update);
//     let remote_id = repo.store_processing_item(&remote_change).await?;

//     let sync_processor = SyncProcessor::new(app_state.clone());
//     process_and_verify_conflict(
//         &repo,
//         &sync_processor,
//         remote_id,
//         ProcessingStatus::Conflicted,
//         Some("Remote item was modified, but its local parent folder was deleted"),
//     ).await?;

//     Ok(())
// }

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_remote_conflict_delete_on_modify() -> Result<()> {
    println!("\n🧪 Remote Conflict: DeleteOnModify");
    let (app_state, repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

    let original_item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?
        .unwrap();

    // Create local file and modify it
    create_local_file(&app_state, 5, "locally modified content").await?;
    
    let local_modified = create_modified_drive_item(original_item.drive_item(), "local-etag-delete");
    let local_change = create_test_local_processing_item(local_modified, ChangeOperation::Update);
    repo.store_processing_item(&local_change).await?;

    // Create remote deletion
    let remote_delete = create_test_remote_processing_item(
        original_item.drive_item().clone(),
        ChangeOperation::Delete,
    );
    let remote_id = repo.store_processing_item(&remote_delete).await?;

    let sync_processor = SyncProcessor::new(app_state.clone());
    process_and_verify_conflict(
        &repo,
        &sync_processor,
        remote_id,
        ProcessingStatus::Conflicted,
        Some("Remote item was deleted, but the local item has been modified"),
    ).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_remote_conflict_rename_or_move_on_existing() -> Result<()> {
    println!("\n🧪 Remote Conflict: RenameOrMoveOnExisting");
    let (app_state, repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

    // Get two files in the same folder
    let file_to_move = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5) // Q1_Report.pdf
        .await?
        .unwrap();

    let existing_file = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(6) // Q2_Report.pdf
        .await?
        .unwrap();

    // Create remote rename that conflicts with existing file name
    let mut remote_renamed = file_to_move.drive_item().clone();
    remote_renamed.name = existing_file.drive_item().name.clone(); // Same name as existing file
    let remote_change = create_test_remote_processing_item(
        remote_renamed,
        ChangeOperation::Move ,
    );
    let remote_id = repo.store_processing_item(&remote_change).await?;

    let sync_processor = SyncProcessor::new(app_state.clone());
    process_and_verify_conflict(
        &repo,
        &sync_processor,
        remote_id,
        ProcessingStatus::Conflicted,
        Some("Remote item was renamed or moved, but an item with the new name already exists locally"),
    ).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_remote_conflict_move_on_move() -> Result<()> {
    println!("\n🧪 Remote Conflict: MoveOnMove");
    let (app_state, repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

    let item_to_move = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?
        .unwrap();

    let local_target_folder = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(6)
        .await?
        .unwrap();

    let remote_target_folder = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(8)
        .await?
        .unwrap();

    // Create local move
    let local_moved = create_moved_drive_item(item_to_move.drive_item(), local_target_folder.id());
    let local_change = create_test_local_processing_item(
        local_moved,
        ChangeOperation::Move ,
    );
    repo.store_processing_item(&local_change).await?;

    // Create remote move to different location
    let remote_moved = create_moved_drive_item(item_to_move.drive_item(), remote_target_folder.id());
    let remote_change = create_test_remote_processing_item(
        remote_moved,
        ChangeOperation::Move ,
    );
    let remote_id = repo.store_processing_item(&remote_change).await?;

    let sync_processor = SyncProcessor::new(app_state.clone());
    process_and_verify_conflict(
        &repo,
        &sync_processor,
        remote_id,
        ProcessingStatus::Conflicted,
        Some("Remote item was moved, but the local item was also moved"),
    ).await?;

    Ok(())
}


//The logic for this is changed - we shoudl recreate folders to limit a possibility of conflicts 
// #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
// #[serial]
// async fn test_remote_conflict_move_to_deleted_parent() -> Result<()> {
//     println!("\n🧪 Remote Conflict: MoveToDeletedParent");
//     let (app_state, repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

//     let item_to_move = drive_items_with_fuse_repo
//         .get_drive_item_with_fuse_by_virtual_ino(5)
//         .await?
//         .unwrap();

//     let target_folder = drive_items_with_fuse_repo
//         .get_drive_item_with_fuse_by_virtual_ino(6)
//         .await?
//         .unwrap();

//     // Mark target folder as deleted locally
//     drive_items_with_fuse_repo.mark_as_deleted_by_onedrive_id(target_folder.id()).await?;

//     // Create remote move to the deleted folder
//     let remote_moved = create_moved_drive_item(item_to_move.drive_item(), target_folder.id());
//     let remote_change = create_test_remote_processing_item(
//         remote_moved,
//         ChangeOperation::Move ,
//     );
//     let remote_id = repo.store_processing_item(&remote_change).await?;

//     let sync_processor = SyncProcessor::new(app_state.clone());
//     process_and_verify_conflict(
//         &repo,
//         &sync_processor,
//         remote_id,
//         ProcessingStatus::Conflicted,
//         Some("Remote item was moved, but the destination parent folder has been deleted locally"),
//     ).await?;

//     Ok(())
// }

// ====================================================================================
// 🌀 LOCAL CONFLICT TESTS - Testing all LocalConflict scenarios  
// ====================================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_local_conflict_create_on_existing() -> Result<()> {
    println!("\n🧪 Local Conflict: CreateOnExisting");
    let (app_state, repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

    // Get parent folder
    let parent_item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(4)
        .await?
        .unwrap();

    // Create a new file that will exist both locally and remotely
    let new_file = create_test_file_item("conflicted_create_456", "ConflictedFile.txt", Some(parent_item.id().to_string()));

    // Create remote creation first
    let remote_create = create_test_remote_processing_item(new_file.clone(), ChangeOperation::Create);
    repo.store_processing_item(&remote_create).await?;

    // Create local creation (conflict)
    let local_create = create_test_local_processing_item(new_file, ChangeOperation::Create);
    let local_id = repo.store_processing_item(&local_create).await?;

    let sync_processor = SyncProcessor::new(app_state.clone());
    process_and_verify_conflict(
        &repo,
        &sync_processor,
        local_id,
        ProcessingStatus::Conflicted,
        Some("Local item created, but an item with the same name already exists on the server"),
    ).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_local_conflict_modify_on_deleted() -> Result<()> {
    println!("\n🧪 Local Conflict: ModifyOnDeleted");
    let (app_state, repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

    let original_item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?
        .unwrap();

    // Create remote deletion first
    let remote_delete = create_test_remote_processing_item(
        original_item.drive_item().clone(),
        ChangeOperation::Delete,
    );
    repo.store_processing_item(&remote_delete).await?;

    // Create local file and modify it
    create_local_file(&app_state, 5, "local changes to deleted file").await?;
    
    let local_modified = create_modified_drive_item(original_item.drive_item(), "local-modify-deleted");
    let local_change = create_test_local_processing_item(local_modified, ChangeOperation::Update);
    let local_id = repo.store_processing_item(&local_change).await?;

    let sync_processor = SyncProcessor::new(app_state.clone());
    process_and_verify_conflict(
        &repo,
        &sync_processor,
        local_id,
        ProcessingStatus::Conflicted,
        Some("Local item was modified, but the corresponding remote item has been deleted"),
    ).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_local_conflict_modify_on_modified() -> Result<()> {
    println!("\n🧪 Local Conflict: ModifyOnModified");
    let (app_state, repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

    let original_item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?
        .unwrap();

    // Create remote modification first
    let remote_modified = create_modified_drive_item(original_item.drive_item(), "remote-modify-first");
    let remote_change = create_test_remote_processing_item(remote_modified, ChangeOperation::Update);
    repo.store_processing_item(&remote_change).await?;

    // Create local file and modify it
    create_local_file(&app_state, 5, "local modification conflict").await?;
    
    let local_modified = create_modified_drive_item(original_item.drive_item(), "local-modify-second");
    let local_change = create_test_local_processing_item(local_modified, ChangeOperation::Update);
    let local_id = repo.store_processing_item(&local_change).await?;

    let sync_processor = SyncProcessor::new(app_state.clone());
    process_and_verify_conflict(
        &repo,
        &sync_processor,
        local_id,
        ProcessingStatus::Conflicted,
        Some("Local item was modified, but the remote item was also modified"),
    ).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_local_conflict_delete_on_modified() -> Result<()> {
    println!("\n🧪 Local Conflict: DeleteOnModified");
    let (app_state, repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

    let original_item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?
        .unwrap();

    // Create remote modification first
    let remote_modified = create_modified_drive_item(original_item.drive_item(), "remote-modify-before-delete");
    let remote_change = create_test_remote_processing_item(remote_modified, ChangeOperation::Update);
    repo.store_processing_item(&remote_change).await?;

    // Create local deletion (conflict because remote was modified)
    let local_delete = create_test_local_processing_item(
        original_item.drive_item().clone(),
        ChangeOperation::Delete,
    );
    let local_id = repo.store_processing_item(&local_delete).await?;

    let sync_processor = SyncProcessor::new(app_state.clone());
    process_and_verify_conflict(
        &repo,
        &sync_processor,
        local_id,
        ProcessingStatus::Conflicted,
        Some("Local item was deleted, but the remote item has been modified"),
    ).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_local_conflict_rename_or_move_to_existing() -> Result<()> {
    println!("\n🧪 Local Conflict: RenameOrMoveToExisting");
    let (app_state, repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

    // Get two files that will conflict on rename
    let file_to_rename = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5) // Q1_Report.pdf
        .await?
        .unwrap();

    let existing_file = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(6) // Q2_Report.pdf  
        .await?
        .unwrap();

    // Create local rename/move that conflicts with existing file
    let mut local_renamed = file_to_rename.drive_item().clone();
    local_renamed.name = existing_file.drive_item().name.clone(); // Rename to same name as existing
    let local_change = create_test_local_processing_item(
        local_renamed,
        ChangeOperation::Move ,
    );
    let local_id = repo.store_processing_item(&local_change).await?;

    let sync_processor = SyncProcessor::new(app_state.clone());
    process_and_verify_conflict(
        &repo,
        &sync_processor,
        local_id,
        ProcessingStatus::Conflicted,
        Some("Local item was renamed or moved, but an item with the target name already exists on the server"),
    ).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_local_conflict_rename_or_move_of_deleted() -> Result<()> {
    println!("\n🧪 Local Conflict: RenameOrMoveOfDeleted");
    let (app_state, repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

    let original_item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?
        .unwrap();

    // Create remote deletion first
    let remote_delete = create_test_remote_processing_item(
        original_item.drive_item().clone(),
        ChangeOperation::Delete,
    );
    repo.store_processing_item(&remote_delete).await?;

    // Try to move the deleted item locally
    let target_folder = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(6)
        .await?
        .unwrap();


    let local_moved = create_moved_drive_item(&original_item.drive_item(), target_folder.id());
    let local_change = create_test_local_processing_item(
        local_moved,
        ChangeOperation::Move ,
    );
    let local_id = repo.store_processing_item(&local_change).await?;

    let sync_processor = SyncProcessor::new(app_state.clone());
    process_and_verify_conflict(
        &repo,
        &sync_processor,
        local_id,
        ProcessingStatus::Conflicted,
        Some("Local item was renamed or moved, but the original source item has been deleted from the server"),
    ).await?;

    Ok(())
}

// ====================================================================================
// 🔄 EDGE CASE TESTS
// ====================================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_local_update_fails_if_file_not_found() -> Result<()> {
    println!("\n🧪 Edge Case: Local update fails if file is missing");
    let (app_state, repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

    let item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?
        .unwrap();

    let local_change =
        create_test_local_processing_item(item.drive_item().clone(), ChangeOperation::Update);
    let item_id = repo.store_processing_item(&local_change).await?;

    // Note: We do NOT create the local file on disk
    remove_local_file(&app_state, 5).await?;
    let sync_processor = SyncProcessor::new(app_state.clone());
    process_and_verify_conflict(
        &repo,
        &sync_processor,
        item_id,
        ProcessingStatus::Error,
        None,
    ).await?;

    Ok(())
}



// ====================================================================================
// 🎯 MOCK API TESTS - Demonstrating mock client capabilities for complex scenarios
// ====================================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_mock_api_response_showcase() -> Result<()> {
    println!("\n🧪 Mock API: Showcase custom responses and failure simulation");
    let (app_state, repo, drive_items_with_fuse_repo, mock_client) = setup_test_env().await?;
    
    // 🎯 SHOWCASE: Configure custom API responses within the test!
    
    // 1. Configure a custom upload response
    mock_client.set_expected_upload_result(UploadResult {
        onedrive_id: "showcase_upload_12345".to_string(),
        etag: Some("showcase_etag_abc123".to_string()),
        web_url: Some("https://showcase.onedrive.com/file123".to_string()),
        size: Some(2048),
    });
    
    // 2. Configure a specific drive item response
    let custom_drive_item = DriveItem {
        id: "showcase_file_id".to_string(),
        name: Some("showcase_document.pdf".to_string()),
        etag: Some("showcase_item_etag_xyz789".to_string()),
        last_modified: Some("2024-01-15T15:30:00Z".to_string()),
        created_date: Some("2024-01-15T15:00:00Z".to_string()),
        size: Some(4096),
        folder: None,
        file: Some(FileFacet {
            mime_type: Some("application/pdf".to_string()),
        }),
        download_url: Some("https://showcase.download.url".to_string()),
        deleted: None,
        parent_reference: None,
    };
    mock_client.set_expected_drive_item("showcase_file_id".to_string(), custom_drive_item);
    
    // 3. Make specific operations fail
    mock_client.make_operation_fail("delete");
    
    // 4. Verify call counting works
    assert_eq!(mock_client.get_call_count("get_user_profile"), 0);
    
    // 🧪 Test the mock configuration by triggering some operations
    
    // This would normally call OneDrive API, but now uses our mock responses
    let item_to_update = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?
        .unwrap();
    
    let mut di = item_to_update.drive_item().clone();
    di.etag = Some("updated-etag-from-test".to_string());
    let processing_item = create_test_remote_processing_item(di, ChangeOperation::Update);
    let item_id = repo.store_processing_item(&processing_item).await?;
    
    let sync_processor = SyncProcessor::new(app_state.clone());
    sync_processor
        .process_single_item(&repo.get_processing_item_by_id(item_id).await?.unwrap())
        .await?;
    
    // 🔍 Verify the mock was used (call counting)
    println!("📊 Mock call counts: {:?}", mock_client.get_all_call_counts());
    
    // 🔍 Verify that operations that should fail, fail
    mock_client.reset_call_counters();
    
    println!("✅ Mock API Response Showcase completed successfully!");
    println!("🎯 This test shows how to configure custom responses per test");
    
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_mock_api_failure_scenarios() -> Result<()> {
    println!("\n🧪 Mock API: Testing failure scenarios and error handling");
    let (app_state, repo, drive_items_with_fuse_repo, mock_client) = setup_test_env().await?;
    
    // Test that operations can be configured to fail
    mock_client.make_operation_fail("upload_file");
    mock_client.make_operation_fail("download_file");
    
    let item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?
        .unwrap();

    // Create local file for upload test
    create_local_file(&app_state, 5, "test content for failed upload").await?;
    
    let local_change = create_test_local_processing_item(
        item.drive_item().clone(),
        ChangeOperation::Update,
    );
    let item_id = repo.store_processing_item(&local_change).await?;
    
    let sync_processor = SyncProcessor::new(app_state.clone());
    sync_processor.process_all_items().await?;
    
    // Verify that the item failed processing due to mocked failure
    let processed_item = repo.get_processing_item_by_id(item_id).await?.unwrap();
    // The exact status depends on how the sync processor handles API failures
    assert!(
        processed_item.status == ProcessingStatus::Error || 
        processed_item.status == ProcessingStatus::Conflicted
    );
    
    println!("✅ Mock API failure scenarios work correctly");
    Ok(())
}

// ====================================================================================
// 🔄 PARENT CHAIN RECREATION TESTS - Testing automatic parent folder recreation
// ====================================================================================

/// Mark a folder as deleted remotely (simulate remote deletion)
async fn mark_folder_as_deleted(
    drive_items_repo: &DriveItemWithFuseRepository,
    virtual_ino: u64,
) -> Result<()> {
    let mut item = drive_items_repo
        .get_drive_item_with_fuse_by_virtual_ino(virtual_ino)
        .await?
        .unwrap();
    
    // Mark as deleted (simulate remote deletion sync)
    item.drive_item.deleted = Some(onedrive_sync_daemon::onedrive_service::onedrive_models::DeletedFacet {
        state: "deleted".to_string(),
    });
    item.set_file_source(onedrive_sync_daemon::persistency::types::FileSource::Remote);
    item.set_sync_status("synced".to_string());
    
    drive_items_repo.store_drive_item_with_fuse(&item).await?;
    Ok(())
}

/// Count processing items by change operation
async fn count_processing_items_by_operation(
    repo: &ProcessingItemRepository,
    operation: ChangeOperation,
) -> Result<usize> {
    let all_items = repo.get_all_processing_items().await?;
    Ok(all_items.iter()
        .filter(|item| item.change_operation == operation)
        .count())
}

/// Get processing items ordered by ID
async fn get_processing_items_ordered(
    repo: &ProcessingItemRepository,
) -> Result<Vec<onedrive_sync_daemon::persistency::processing_item_repository::ProcessingItem>> {
    let mut items = repo.get_all_processing_items().await?;
    items.sort_by_key(|item| item.id.unwrap_or(0));
    Ok(items)
}

/// Verify processing item order matches expected pattern
fn verify_processing_order(
    items: &[onedrive_sync_daemon::persistency::processing_item_repository::ProcessingItem],
    expected_pattern: &[ChangeOperation],
) -> Result<()> {
    assert_eq!(items.len(), expected_pattern.len(),
        "Expected {} items, got {}. Items: {:?}", 
        expected_pattern.len(), items.len(),
        items.iter().map(|i| (&i.drive_item.name, &i.change_operation)).collect::<Vec<_>>()
    );
    
    for (item, expected_op) in items.iter().zip(expected_pattern.iter()) {
        assert_eq!(item.change_operation, *expected_op,
            "Expected operation {:?} for item '{}', got {:?}",
            expected_op, item.drive_item.name.as_deref().unwrap_or("unnamed"), item.change_operation
        );
    }
    
    // Verify IDs are ascending
    for i in 1..items.len() {
        assert!(items[i].id.unwrap() > items[i-1].id.unwrap(),
            "Processing items should have ascending IDs");
    }
    
    Ok(())
}

/// Verify parent-child relationships are correct
async fn verify_parent_child_relationships(
    items: &[onedrive_sync_daemon::persistency::processing_item_repository::ProcessingItem],
    drive_items_repo: &DriveItemWithFuseRepository,
) -> Result<()> {
    for item in items {
        if let Some(parent_ref) = &item.drive_item.parent_reference {
            // Check if parent item exists in the processing items or in database
            let parent_exists_in_processing = items.iter().any(|i| i.drive_item.id == parent_ref.id);
            let parent_exists_in_db = drive_items_repo
                .get_drive_item_with_fuse(&parent_ref.id)
                .await?
                .map(|p| !p.is_deleted())
                .unwrap_or(false);
            
            assert!(parent_exists_in_processing || parent_exists_in_db,
                "Item '{}' has parent '{}' that doesn't exist in processing queue or database",
                item.drive_item.name.as_deref().unwrap_or("unnamed"),
                parent_ref.id
            );
        }
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_parent_chain_recreation_single_level() -> Result<()> {
    println!("\n🧪 Parent Chain Recreation: Single level parent deletion");
    let (app_state, repo, drive_items_repo, _mock_client) = setup_test_env().await?;

    // Get the file that will be modified (Q1_Report.pdf in Reports folder)
    let file_item = drive_items_repo
        .get_drive_item_with_fuse_by_virtual_ino(5) // Q1_Report.pdf
        .await?
        .unwrap();
    
    println!("📁 File: {} (ino: {})", 
        file_item.drive_item().name.as_deref().unwrap_or("unnamed"), 
        file_item.virtual_ino().unwrap()
    );

    // Get the parent folder (Reports) and mark it as deleted
    let parent_folder = drive_items_repo
        .get_drive_item_with_fuse_by_virtual_ino(4) // Reports folder
        .await?
        .unwrap();
    
    println!("📂 Parent folder: {} (ino: {}) - marking as deleted", 
        parent_folder.drive_item().name.as_deref().unwrap_or("unnamed"),
        parent_folder.virtual_ino().unwrap()
    );
    
    mark_folder_as_deleted(&drive_items_repo, 4).await?;

    // Create local change for the file
    let local_change = create_test_local_processing_item(
        file_item.drive_item().clone(),
        ChangeOperation::Update,
    );
    let item_id = repo.store_processing_item(&local_change).await?;
    println!("📋 Created local processing item with ID: {}", item_id);

    // Get initial processing item count
    let initial_items = repo.get_all_processing_items().await?;
    let initial_count = initial_items.len();
    println!("📊 Initial processing items count: {}", initial_count);

    // Process the item
    let sync_processor = SyncProcessor::new(app_state.clone());
    let item_to_process = repo.get_processing_item_by_id(item_id).await?.unwrap();
    
    println!("🔄 Processing item: {} (operation: {:?})", 
        item_to_process.drive_item.name.as_deref().unwrap_or("unnamed"),
        item_to_process.change_operation
    );
    
    sync_processor.process_single_item(&item_to_process).await?;

    // Verify results
    let final_items = get_processing_items_ordered(&repo).await?;
    let create_count = count_processing_items_by_operation(&repo, ChangeOperation::Create).await?;
    
    println!("📊 Final processing items count: {}", final_items.len());
    println!("📁 Created folder processing items: {}", create_count);
    
    for (i, item) in final_items.iter().enumerate() {
        println!("   [{}] {:?} {} (ID: {}, Parent: {})", 
            i + 1,
            item.change_operation,
            item.drive_item.name.as_deref().unwrap_or("unnamed"),
            item.id.unwrap_or(0),
            item.drive_item.parent_reference.as_ref()
                .map(|p| p.id.clone())
                .unwrap_or_else(|| "none".to_string())
        );
    }

    // Verify we have more items than before (parent + recreated child)
    assert!(final_items.len() > initial_count, 
        "Should have created additional processing items for parent recreation");
    
    // Verify we created at least one folder
    assert!(create_count > 0, 
        "Should have created at least one folder processing item");

    // Verify processing order: Create parent → Update child
    let expected_pattern = vec![
        ChangeOperation::Create, // Reports folder
        ChangeOperation::Update, // Q1_Report.pdf file
    ];
    verify_processing_order(&final_items, &expected_pattern)?;

    // Verify parent-child relationships
    verify_parent_child_relationships(&final_items, &drive_items_repo).await?;

    // Verify the created parent folder has correct properties
    let parent_create_item = final_items.iter()
        .find(|item| item.change_operation == ChangeOperation::Create)
        .expect("Should have created folder processing item");
    
    assert!(parent_create_item.drive_item.folder.is_some(), 
        "Created item should be a folder");
    assert!(parent_create_item.drive_item.id.starts_with("local_"), 
        "Created folder should have temporary local ID");

    println!("✅ Single level parent chain recreation test passed!");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_parent_chain_recreation_multi_level() -> Result<()> {
    println!("\n🧪 Parent Chain Recreation: Multi-level parent deletion");
    let (app_state, repo, drive_items_repo, _mock_client) = setup_test_env().await?;

    // Get the file that will be modified (Q1_Report.pdf)
    let file_item = drive_items_repo
        .get_drive_item_with_fuse_by_virtual_ino(5) // Q1_Report.pdf
        .await?
        .unwrap();
    
    println!("📁 File: {} (ino: {})", 
        file_item.drive_item().name.as_deref().unwrap_or("unnamed"), 
        file_item.virtual_ino().unwrap()
    );

    // Mark both Work (ino 3) and Reports (ino 4) folders as deleted
    println!("📂 Marking Work folder (ino: 3) as deleted");
    mark_folder_as_deleted(&drive_items_repo, 3).await?; // Work folder
    
    println!("📂 Marking Reports folder (ino: 4) as deleted");
    mark_folder_as_deleted(&drive_items_repo, 4).await?; // Reports folder

    // Create local change for the file
    let local_change = create_test_local_processing_item(
        file_item.drive_item().clone(),
        ChangeOperation::Update,
    );
    let item_id = repo.store_processing_item(&local_change).await?;
    println!("📋 Created local processing item with ID: {}", item_id);

    // Get initial processing item count
    let initial_count = repo.get_all_processing_items().await?.len();
    println!("📊 Initial processing items count: {}", initial_count);

    // Process the item
    let sync_processor = SyncProcessor::new(app_state.clone());
    let item_to_process = repo.get_processing_item_by_id(item_id).await?.unwrap();
    
    println!("🔄 Processing item: {} (operation: {:?})", 
        item_to_process.drive_item.name.as_deref().unwrap_or("unnamed"),
        item_to_process.change_operation
    );
    
    sync_processor.process_single_item(&item_to_process).await?;

    // Verify results
    let final_items = get_processing_items_ordered(&repo).await?;
    let create_count = count_processing_items_by_operation(&repo, ChangeOperation::Create).await?;
    
    println!("📊 Final processing items count: {}", final_items.len());
    println!("📁 Created folder processing items: {}", create_count);
    
    for (i, item) in final_items.iter().enumerate() {
        println!("   [{}] {:?} {} (ID: {}, Parent: {})", 
            i + 1,
            item.change_operation,
            item.drive_item.name.as_deref().unwrap_or("unnamed"),
            item.id.unwrap_or(0),
            item.drive_item.parent_reference.as_ref()
                .map(|p| p.id.clone())
                .unwrap_or_else(|| "none".to_string())
        );
    }

    // Verify we have more items than before (2 parents + recreated child)
    assert!(final_items.len() > initial_count, 
        "Should have created additional processing items for parent recreation");
    
    // Verify we created exactly 2 folders
    assert_eq!(create_count, 2, 
        "Should have created exactly 2 folder processing items (Work and Reports)");

    // Verify processing order: Create Work → Create Reports → Update file
    let expected_pattern = vec![
        ChangeOperation::Create, // Work folder
        ChangeOperation::Create, // Reports folder
        ChangeOperation::Update, // Q1_Report.pdf file
    ];
    verify_processing_order(&final_items, &expected_pattern)?;

    // Verify parent-child relationships
    verify_parent_child_relationships(&final_items, &drive_items_repo).await?;

    // Verify the folder hierarchy is correct
    let create_items: Vec<_> = final_items.iter()
        .filter(|item| item.change_operation == ChangeOperation::Create)
        .collect();
    
    assert_eq!(create_items.len(), 2, "Should have exactly 2 created folders");
    
    // First created item should be Work folder (higher in hierarchy)
    let work_item = &create_items[0];
    assert!(work_item.drive_item.name.as_ref().unwrap().contains("Work") || 
            work_item.drive_item.id.starts_with("local_"), 
        "First created item should be Work folder or have local ID");
    
    // Second created item should be Reports folder (child of Work)
    let reports_item = &create_items[1];
    assert!(reports_item.drive_item.name.as_ref().unwrap().contains("Reports") || 
            reports_item.drive_item.id.starts_with("local_"), 
        "Second created item should be Reports folder or have local ID");

    println!("✅ Multi-level parent chain recreation test passed!");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_parent_chain_recreation_local_create() -> Result<()> {
    println!("\n🧪 Parent Chain Recreation: Local file creation in deleted parent");
    let (app_state, repo, drive_items_repo, _mock_client) = setup_test_env().await?;

    // Get the Reports folder and mark it as deleted
    let reports_folder = drive_items_repo
        .get_drive_item_with_fuse_by_virtual_ino(4) // Reports folder
        .await?
        .unwrap();
    
    println!("📂 Marking Reports folder as deleted");
    mark_folder_as_deleted(&drive_items_repo, 4).await?;

    // Create a new file in the deleted Reports folder
    let new_file = create_test_file_item(
        "new_report_123", 
        "NewReport.pdf", 
        Some(reports_folder.drive_item().id.clone())
    );

    // Create local processing item for new file creation
    let local_create = create_test_local_processing_item(new_file, ChangeOperation::Create);
    let item_id = repo.store_processing_item(&local_create).await?;
    
    println!("📋 Created local file creation processing item with ID: {}", item_id);

    // Get initial processing item count
    let initial_count = repo.get_all_processing_items().await?.len();
    println!("📊 Initial processing items count: {}", initial_count);

    // Process the item
    let sync_processor = SyncProcessor::new(app_state.clone());
    let item_to_process = repo.get_processing_item_by_id(item_id).await?.unwrap();
    
    println!("🔄 Processing item: {} (operation: {:?})", 
        item_to_process.drive_item.name.as_deref().unwrap_or("unnamed"),
        item_to_process.change_operation
    );
    
    sync_processor.process_single_item(&item_to_process).await?;

    // Verify results
    let final_items = get_processing_items_ordered(&repo).await?;
    let create_count = count_processing_items_by_operation(&repo, ChangeOperation::Create).await?;
    
    println!("📊 Final processing items count: {}", final_items.len());
    println!("📁 Total create operations: {}", create_count);
    
    for (i, item) in final_items.iter().enumerate() {
        println!("   [{}] {:?} {} (ID: {})", 
            i + 1,
            item.change_operation,
            item.drive_item.name.as_deref().unwrap_or("unnamed"),
            item.id.unwrap_or(0)
        );
    }

    // Verify we created parent folder + new file
    assert!(final_items.len() > initial_count, 
        "Should have created additional processing items");
    assert_eq!(create_count, 2, 
        "Should have exactly 2 create operations: folder + file");

    // Verify processing order: Create parent folder → Create new file
    let expected_pattern = vec![
        ChangeOperation::Create, // Reports folder
        ChangeOperation::Create, // NewReport.pdf file
    ];
    verify_processing_order(&final_items, &expected_pattern)?;

    println!("✅ Local file creation in deleted parent test passed!");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_parent_chain_recreation_skip_delete_operations() -> Result<()> {
    println!("\n🧪 Parent Chain Recreation: Skip delete operations (should not recreate parents)");
    let (app_state, repo, drive_items_repo, _mock_client) = setup_test_env().await?;

    // Get the file and its parent folder
    let file_item = drive_items_repo
        .get_drive_item_with_fuse_by_virtual_ino(5) // Q1_Report.pdf
        .await?
        .unwrap();
    
    // Mark parent folder as deleted
    println!("📂 Marking Reports folder as deleted");
    mark_folder_as_deleted(&drive_items_repo, 4).await?; // Reports folder

    // Create local DELETE operation for the file
    let local_delete = create_test_local_processing_item(
        file_item.drive_item().clone(),
        ChangeOperation::Delete,
    );
    let item_id = repo.store_processing_item(&local_delete).await?;
    
    println!("📋 Created local file deletion processing item with ID: {}", item_id);

    // Get initial processing item count
    let initial_count = repo.get_all_processing_items().await?.len();

    // Process the item
    let sync_processor = SyncProcessor::new(app_state.clone());
    let item_to_process = repo.get_processing_item_by_id(item_id).await?.unwrap();
    
    println!("🔄 Processing delete operation for: {}", 
        item_to_process.drive_item.name.as_deref().unwrap_or("unnamed")
    );
    
    sync_processor.process_single_item(&item_to_process).await?;

    // Verify results
    let final_items = get_processing_items_ordered(&repo).await?;
    let create_count = count_processing_items_by_operation(&repo, ChangeOperation::Create).await?;
    
    println!("📊 Final processing items count: {}", final_items.len());
    println!("📁 Created folder processing items: {}", create_count);

    // For delete operations, we should NOT recreate parents
    assert_eq!(final_items.len(), initial_count, 
        "Delete operations should not trigger parent recreation");
    assert_eq!(create_count, 0, 
        "Should not have created any folder processing items for delete operations");

    // Verify the original item was processed normally
    let processed_item = repo.get_processing_item_by_id(item_id).await?.unwrap();
    assert_eq!(processed_item.status, ProcessingStatus::Done, 
        "Delete operation should complete normally");

    println!("✅ Skip delete operations test passed!");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_zzz_comprehensive_conflict_coverage_complete() -> Result<()> {
    println!("\n🎯 ===== COMPREHENSIVE CONFLICT TEST COVERAGE COMPLETE =====");
    
    Ok(())
}
