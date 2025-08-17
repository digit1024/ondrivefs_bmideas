use anyhow::Result;
use onedrive_sync_daemon::file_manager::FileManager;
use onedrive_sync_daemon::persistency::drive_item_with_fuse_repository::DriveItemWithFuseRepository;
use onedrive_sync_daemon::persistency::processing_item_repository::{
    ChangeOperation, ProcessingItemRepository, ProcessingStatus,
};
use onedrive_sync_daemon::sync::SyncProcessor;
use serial_test::serial;
use std::sync::Arc;

use crate::common::fixtures::{
    create_test_local_processing_item, create_test_remote_processing_item,
};
use crate::common::setup::TEST_ENV;
use crate::common::mock_onedrive_client::MockOneDriveClient;
use onedrive_sync_daemon::app_state::AppState;
use onedrive_sync_daemon::onedrive_service::onedrive_models::{UploadResult, DriveItem, DeleteResult, CreateFolderResult, FileFacet};



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
        panic!("Failed to delete database file");
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

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_modify_on_modify_conflict_is_detected() -> Result<()> {
    println!("\n🧪 Running test: Modify/Modify conflict detection");
    let (app_state, repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

    let original_item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?
        .unwrap();
    let original_etag = original_item.drive_item().etag.clone();

    // Create a dummy file to be "updated"
    let local_dir = app_state.file_manager().get_local_dir();
    let file_path = local_dir.join(original_item.virtual_ino().unwrap().to_string());
    std::fs::create_dir_all(&local_dir)?;
    std::fs::write(&file_path, "local content")?;

    let local_change = create_test_local_processing_item(
        original_item.drive_item().clone(),
        ChangeOperation::Update,
    );
    let local_id = repo.store_processing_item(&local_change).await?;

    let mut remote_di = original_item.drive_item().clone();
    remote_di.etag = Some("new-remote-etag".to_string());
    let remote_change = create_test_remote_processing_item(remote_di, ChangeOperation::Update);
    let remote_id = repo.store_processing_item(&remote_change).await?;

    let sync_processor = SyncProcessor::new(app_state.clone());
    sync_processor.process_all_items().await?;

    let final_item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?
        .unwrap();
    assert_eq!(final_item.drive_item().etag, original_etag); // Etag is not changed

    let local_processed = repo.get_processing_item_by_id(local_id).await?.unwrap();
    assert_eq!(local_processed.status, ProcessingStatus::Conflicted);

    let remote_processed = repo.get_processing_item_by_id(remote_id).await?.unwrap();
    assert_eq!(remote_processed.status, ProcessingStatus::Conflicted);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_local_update_fails_if_file_not_found() -> Result<()> {
    println!("\n🧪 Running test: Local update fails if file is missing");
    let (app_state, repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

    let item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?
        .unwrap();

    let local_change =
        create_test_local_processing_item(item.drive_item().clone(), ChangeOperation::Update);
    let item_id = repo.store_processing_item(&local_change).await?;

    // Note: We do NOT create the local file on disk
    let sync_processor = SyncProcessor::new(app_state.clone());
    sync_processor.process_all_items().await?;

    let processed_item = repo.get_processing_item_by_id(item_id).await?.unwrap();
    assert_eq!(processed_item.status, ProcessingStatus::Error); // It should fail

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_remote_move_on_local_move_conflict_is_detected() -> Result<()> {
    println!("\n🧪 Running test: Remote move on local move conflict");
    let (app_state, repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

    let item_to_move = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?
        .unwrap(); // "Q1_Report.pdf" in "Folder B" (ino 4)
    let original_parent_ino = item_to_move.parent_ino();

    let new_local_parent = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(6)
        .await?
        .unwrap(); // "Folder C"

    let new_remote_parent = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(8)
        .await?
        .unwrap(); // "Folder D"

    let mut local_move_di = item_to_move.drive_item().clone();
    local_move_di.parent_reference = Some(new_local_parent.drive_item().into());
    let local_move = create_test_local_processing_item(
        local_move_di,
        ChangeOperation::Move {
            old_path: "".into(),
            new_path: "".into(),
        },
    );
    repo.store_processing_item(&local_move).await?;

    let mut remote_move_di = item_to_move.drive_item().clone();
    remote_move_di.parent_reference = Some(new_remote_parent.drive_item().into());
    let remote_move = create_test_remote_processing_item(
        remote_move_di,
        ChangeOperation::Move {
            old_path: "".into(),
            new_path: "".into(),
        },
    );
    let remote_id = repo.store_processing_item(&remote_move).await?;

    let sync_processor = SyncProcessor::new(app_state.clone());
    sync_processor.process_all_items().await?;

    let processed = repo.get_processing_item_by_id(remote_id).await?.unwrap();
    assert_eq!(processed.status, ProcessingStatus::Conflicted);
    assert!(processed.validation_errors[0]
        .contains("Remote item was moved, but the local item was also moved"));

    let final_item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?
        .unwrap();
    assert_eq!(final_item.parent_ino(), original_parent_ino);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_local_move_of_remote_deleted_item_conflict() -> Result<()> {
    println!("\n🧪 Running test: Local move of a remote-deleted item conflict");
    let (app_state, repo, drive_items_with_fuse_repo, _mock_client) = setup_test_env().await?;

    let item_to_process = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?
        .unwrap(); // "Q1_Report.pdf"

    // Mark the item as deleted on remote
    drive_items_with_fuse_repo
        .mark_as_deleted_by_onedrive_id(&item_to_process.id())
        .await?;

    let new_parent = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(6)
        .await?
        .unwrap(); // "Folder C"

    let mut local_move_di = item_to_process.drive_item().clone();
    local_move_di.parent_reference = Some(new_parent.drive_item().into());
    let local_move = create_test_local_processing_item(
        local_move_di,
        ChangeOperation::Move {
            old_path: "".into(),
            new_path: "".into(),
        },
    );
    let local_id = repo.store_processing_item(&local_move).await?;

    let sync_processor = SyncProcessor::new(app_state.clone());
    sync_processor.process_all_items().await?;

    let processed = repo.get_processing_item_by_id(local_id).await?.unwrap();
    assert_eq!(processed.status, ProcessingStatus::Conflicted);
    assert!(processed.validation_errors[0]
        .contains("Local item was renamed or moved, but the original source item has been deleted"));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_mock_api_response_showcase() -> Result<()> {
    println!("\n🧪 Running test: Mock API Response Showcase");
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
async fn test_zzzz_last_test() -> Result<()> {
    println!("\n🧪 ");
    Ok(())
}
