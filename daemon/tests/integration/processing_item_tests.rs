use anyhow::Result;
use onedrive_sync_daemon::persistency::processing_item_repository::{
    ChangeOperation, ProcessingStatus,
};
use serial_test::serial;

// Import from our test modules
use crate::common::fixtures::{
    create_test_file_item, create_test_local_processing_item, create_test_processing_item,
    create_test_processing_item_with_status, create_test_remote_processing_item,
};
use crate::common::setup::TEST_ENV;

/// Test storing and retrieving a processing item by ID
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_fs_item_tree_creation_works() -> Result<()> {
    println!("\n🧪 Running test: Clash of local and remote changes");
    // SETUP
    // Get the shared test environment and AppState
    let mut env = TEST_ENV.lock().await;
    let app_state = env.get_app_state().await?;
    env.clear_all_data().await?;
    // Get the processing item repository
    let repo = app_state.persistency().processing_item_repository();

    // GIVEN
    let tree_items = crate::common::fixtures::create_drive_items_tree(); // ~50 items with valid relationships
    let drive_items_with_fuse_repo = app_state.persistency().drive_item_with_fuse_repository();
    for item in &tree_items {
        drive_items_with_fuse_repo
            .store_drive_item_with_fuse(&item)
            .await?;
    }
    //WHEN
    let item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?;
    assert_eq!(item.is_some(), true);
    let item = item.unwrap();

    assert_eq!(item.drive_item().name, Some("Q1_Report.pdf".to_string()));
    assert_eq!(item.parent_ino(), Some(4));
    assert_eq!(item.drive_item().file.is_some(), true);

    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_processing_item_modified_no_conflicts() -> Result<()> {
    println!("\n🧪 Running test: Remote change that goes well");
    // SETUP
    // Get the shared test environment and AppState
    let mut env = TEST_ENV.lock().await;
    let app_state = env.get_app_state().await?;
    env.clear_all_data().await?;
    // Get the processing item repository
    let repo = app_state.persistency().processing_item_repository();

    // GIVEN
    let tree_items = crate::common::fixtures::create_drive_items_tree(); // ~50 items with valid relationships
    let drive_items_with_fuse_repo = app_state.persistency().drive_item_with_fuse_repository();
    for item in &tree_items {
        drive_items_with_fuse_repo
            .store_drive_item_with_fuse(&item)
            .await?;
    }
    //WHEN
    let item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?;
    assert_eq!(item.is_some(), true);
    let item = item.unwrap();
    // AND Item is modified remotely
    let mut di = item.drive_item().clone();
    di.etag = Some("12345dasd7890".to_string()); // new Etag =  modified
    let processing_item = create_test_remote_processing_item(di, ChangeOperation::Update);
    let stored_id = repo.store_processing_item(&processing_item).await?;
    // And  the item is beeing processed
    let processing_item = repo.get_processing_item_by_id(stored_id).await?;
    let sync_processor = onedrive_sync_daemon::sync::SyncProcessor::new(app_state);
    sync_processor
        .process_single_item(&processing_item.unwrap())
        .await?;
    // THEN
    // The item is  modified
    let item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?;
    assert_eq!(item.is_some(), true);
    let item = item.unwrap();
    assert_eq!(item.drive_item().etag, Some("12345dasd7890".to_string())); // Etag is updated
    assert_eq!(item.drive_item().name, Some("Q1_Report.pdf".to_string())); // Name is not modified
    assert_eq!(item.parent_ino(), Some(4)); // Parent is not modified
    assert_eq!(item.drive_item().file.is_some(), true); // File is not modified
                                                        // And The processing item is done
    let processing_item = repo.get_processing_item_by_id(stored_id).await?;
    assert_eq!(processing_item.unwrap().status, ProcessingStatus::Done);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_processing_item_modified_locally_and_remotely() -> Result<()> {
    println!("\n🧪 Running test: Clash of local and remote changes");
    // SETUP
    // Get the shared test environment and AppState
    let mut env = TEST_ENV.lock().await;
    let app_state = env.get_app_state().await?;
    env.clear_all_data().await?;
    // Get the processing item repository
    let repo = app_state.persistency().processing_item_repository();

    // GIVEN
    let tree_items = crate::common::fixtures::create_drive_items_tree(); // ~50 items with valid relationships
    let drive_items_with_fuse_repo = app_state.persistency().drive_item_with_fuse_repository();
    for item in &tree_items {
        drive_items_with_fuse_repo
            .store_drive_item_with_fuse(&item)
            .await?;
    }
    //WHEN
    let original_item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?;
    assert_eq!(original_item.is_some(), true);
    let item = original_item.clone().unwrap();
    // AND Item is modified locally
    let mut di = item.drive_item().clone();
    let processing_item = create_test_local_processing_item(di.clone(), ChangeOperation::Update);
    let local_processing_item_id = repo.store_processing_item(&processing_item).await?;
    // AND Item is modified remotely (CONFLICT)
    di.etag = Some("12345dasd7890".to_string()); // new Etag =  modified
    let processing_item = create_test_remote_processing_item(di, ChangeOperation::Update);
    let stored_id = repo.store_processing_item(&processing_item).await?;

    // And  the item is beeing processed
    let remote_processing_item = repo.get_processing_item_by_id(stored_id).await?;
    let local_processing_item = repo
        .get_processing_item_by_id(local_processing_item_id)
        .await?;
    let sync_processor = onedrive_sync_daemon::sync::SyncProcessor::new(app_state);
    sync_processor
        .process_single_item(&remote_processing_item.unwrap())
        .await?;
    sync_processor
        .process_single_item(&local_processing_item.unwrap())
        .await?;
    // THEN
    // The item is NOT modified
    let item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?;
    assert_eq!(item.is_some(), true);
    let item = item.unwrap();
    assert_eq!(
        item.drive_item().etag,
        original_item.unwrap().drive_item().etag
    ); // Etag is NOT updated
    assert_eq!(item.drive_item().name, Some("Q1_Report.pdf".to_string())); // Name is not modified
    assert_eq!(item.parent_ino(), Some(4)); // Parent is not modified
    assert_eq!(item.drive_item().file.is_some(), true); // File is not modified
                                                        // And The processing item is Conflict
    let processing_item = repo.get_processing_item_by_id(stored_id).await?;
    assert_eq!(
        processing_item.unwrap().status,
        ProcessingStatus::Conflicted
    );
    let processing_item = repo
        .get_processing_item_by_id(local_processing_item_id)
        .await?;
    assert_eq!(
        processing_item.unwrap().status,
        ProcessingStatus::Conflicted
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_processing_item_modified_locally_and_remotely_all_items_process() -> Result<()> {
    println!("\n🧪 Running test: Clash of local and remote changes");
    // SETUP
    // Get the shared test environment and AppState
    let mut env = TEST_ENV.lock().await;
    let app_state = env.get_app_state().await?;
    env.clear_all_data().await?;
    // Get the processing item repository
    let repo = app_state.persistency().processing_item_repository();

    // GIVEN
    let tree_items = crate::common::fixtures::create_drive_items_tree(); // ~50 items with valid relationships
    let drive_items_with_fuse_repo = app_state.persistency().drive_item_with_fuse_repository();
    for item in &tree_items {
        drive_items_with_fuse_repo
            .store_drive_item_with_fuse(&item)
            .await?;
    }
    //WHEN
    let original_item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?;
    assert_eq!(original_item.is_some(), true);
    let item = original_item.clone().unwrap();
    // AND Item is modified locally
    let mut di = item.drive_item().clone();
    let processing_item = create_test_local_processing_item(di.clone(), ChangeOperation::Update);
    let local_processing_item_id = repo.store_processing_item(&processing_item).await?;
    // AND Item is modified remotely (CONFLICT)
    di.etag = Some("12345dasd7890".to_string()); // new Etag =  modified
    let processing_item = create_test_remote_processing_item(di, ChangeOperation::Update);
    let stored_id = repo.store_processing_item(&processing_item).await?;

    // And  the item is beeing processed
    let remote_processing_item = repo.get_processing_item_by_id(stored_id).await?;
    let local_processing_item = repo
        .get_processing_item_by_id(local_processing_item_id)
        .await?;
    let sync_processor = onedrive_sync_daemon::sync::SyncProcessor::new(app_state);
    sync_processor.process_all_items().await?;
    // THEN
    // The item is NOT modified
    let item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?;
    assert_eq!(item.is_some(), true);
    let item = item.unwrap();
    assert_eq!(
        item.drive_item().etag,
        original_item.unwrap().drive_item().etag
    ); // Etag is NOT updated
    assert_eq!(item.drive_item().name, Some("Q1_Report.pdf".to_string())); // Name is not modified
    assert_eq!(item.parent_ino(), Some(4)); // Parent is not modified
    assert_eq!(item.drive_item().file.is_some(), true); // File is not modified
                                                        // And The processing item is Conflict
    let processing_item = repo.get_processing_item_by_id(stored_id).await?;
    assert_eq!(
        processing_item.unwrap().status,
        ProcessingStatus::Conflicted
    );
    let processing_item = repo
        .get_processing_item_by_id(local_processing_item_id)
        .await?;
    assert_eq!(
        processing_item.unwrap().status,
        ProcessingStatus::Conflicted
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_processing_item_modified_locally_should_fail_file_not_found() -> Result<()> {
    println!("\n🧪 Running test: Clash of local and remote changes");
    // SETUP
    // Get the shared test environment and AppState
    let mut env = TEST_ENV.lock().await;
    let app_state = env.get_app_state().await?;
    env.clear_all_data().await?;
    // Get the processing item repository
    let repo = app_state.persistency().processing_item_repository();

    // GIVEN
    let tree_items = crate::common::fixtures::create_drive_items_tree(); // ~50 items with valid relationships
    let drive_items_with_fuse_repo = app_state.persistency().drive_item_with_fuse_repository();
    for item in &tree_items {
        drive_items_with_fuse_repo
            .store_drive_item_with_fuse(&item)
            .await?;
    }
    //WHEN
    let original_item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?;
    assert_eq!(original_item.is_some(), true);
    let item = original_item.clone().unwrap();
    // AND Item is modified locally
    let di = item.drive_item().clone();
    let processing_item = create_test_local_processing_item(di.clone(), ChangeOperation::Update);
    let local_processing_item_id = repo.store_processing_item(&processing_item).await?;

    // And  the item is beeing processed

    let local_processing_item = repo
        .get_processing_item_by_id(local_processing_item_id)
        .await?;
    let sync_processor = onedrive_sync_daemon::sync::SyncProcessor::new(app_state);
    sync_processor.process_all_items().await?;
    // THEN
    // The item is NOT modified
    let item = drive_items_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_ino(5)
        .await?;
    assert_eq!(item.is_some(), true);
    let item = item.unwrap();
    assert_eq!(
        item.drive_item().etag,
        original_item.unwrap().drive_item().etag
    ); // Etag is NOT updated
    assert_eq!(item.drive_item().name, Some("Q1_Report.pdf".to_string())); // Name is not modified
    assert_eq!(item.parent_ino(), Some(4)); // Parent is not modified
    assert_eq!(item.drive_item().file.is_some(), true); // File is not modified
                                                        // And The processing item is In error

    let processing_item = repo
        .get_processing_item_by_id(local_processing_item_id)
        .await?;
    assert_eq!(processing_item.unwrap().status, ProcessingStatus::Error);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn test_zzzz_last_test() -> Result<()> {
    println!("\n🧪 ");

    Ok(())
}
