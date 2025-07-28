use anyhow::Result;
use serial_test::serial;
use onedrive_sync_daemon::persistency::processing_item_repository::{
    ProcessingStatus, ChangeOperation
};

// Import from our test modules
use crate::common::fixtures::{
    create_test_file_item, create_test_processing_item,
    create_test_processing_item_with_status
};
use crate::common::setup::TEST_ENV;

/// Test storing and retrieving a processing item by ID
#[tokio::test]
#[serial]
async fn test_get_processing_item_by_id() -> Result<()> {
    println!("\n🧪 Running test: get_processing_item_by_id");
    
    // Get the shared test environment and AppState
    let mut env = TEST_ENV.lock().await;
    let app_state = env.get_app_state().await?;
    
    // Get the processing item repository
    let repo = app_state.persistency().processing_item_repository();
    
    // Create a test file item
    let drive_item = create_test_file_item("test_file_001", "test_document.txt", None);
    let processing_item = create_test_processing_item(drive_item);
    
    // Store the processing item
    let stored_id = repo.store_processing_item(&processing_item).await?;
    println!("✅ Stored processing item with database ID: {}", stored_id);
    
    // Retrieve the item by ID
    let retrieved_item = repo.get_processing_item_by_id(stored_id).await?;
    
    // Verify the item was retrieved
    assert!(retrieved_item.is_some(), "Processing item should be found");
    let retrieved_item = retrieved_item.unwrap();
    
    // Verify the data matches
    assert_eq!(retrieved_item.id, Some(stored_id));
    assert_eq!(retrieved_item.drive_item.id, "test_file_001");
    assert_eq!(retrieved_item.drive_item.name, Some("test_document.txt".to_string()));
    assert_eq!(retrieved_item.status, ProcessingStatus::New);
    
    println!("✅ Successfully retrieved processing item by ID");
    
    Ok(())
}

/// Test that database persists between tests
#[tokio::test]
#[serial]
async fn test_database_persistence_between_tests() -> Result<()> {
    println!("\n🧪 Running test: database_persistence_between_tests");
    
    // Get the shared test environment and AppState
    let mut env = TEST_ENV.lock().await;
    let app_state = env.get_app_state().await?;
    
    // Get the processing item repository
    let repo = app_state.persistency().processing_item_repository();
    
    // Try to retrieve the item stored in the previous test
    let items = repo.get_all_processing_items().await?;
    
    // We should have at least one item from the previous test
    assert!(!items.is_empty(), "Database should contain items from previous test");
    
    // Find our test item
    let test_item = items.iter().find(|item| item.drive_item.id == "test_file_001");
    assert!(test_item.is_some(), "Should find the item from previous test");
    
    println!("✅ Database successfully persisted data between tests");
    println!("📊 Total items in database: {}", items.len());
    
    Ok(())
}

/// Test storing multiple items and retrieving by status
#[tokio::test]
#[serial]
async fn test_get_processing_items_by_status() -> Result<()> {
    println!("\n🧪 Running test: get_processing_items_by_status");
    
    // Get the shared test environment and AppState
    let mut env = TEST_ENV.lock().await;
    let app_state = env.get_app_state().await?;
    
    // Get the processing item repository
    let repo = app_state.persistency().processing_item_repository();
    
    // Create items with different statuses
    let statuses = vec![
        ("file_validated_001", ProcessingStatus::Validated),
        ("file_processing_001", ProcessingStatus::Processing),
        ("file_done_001", ProcessingStatus::Done),
        ("file_validated_002", ProcessingStatus::Validated),
    ];
    
    for (id, status) in statuses {
        let drive_item = create_test_file_item(id, &format!("{}.txt", id), None);
        let processing_item = create_test_processing_item_with_status(drive_item, status);
        repo.store_processing_item(&processing_item).await?;
        println!("📝 Stored item {} with status {:?}", id, status);
    }
    
    // Query items by status
    let validated_items = repo.get_processing_items_by_status(&ProcessingStatus::Validated).await?;
    assert_eq!(validated_items.len(), 2, "Should have 2 validated items");
    
    let processing_items = repo.get_processing_items_by_status(&ProcessingStatus::Processing).await?;
    assert_eq!(processing_items.len(), 1, "Should have 1 processing item");
    
    let done_items = repo.get_processing_items_by_status(&ProcessingStatus::Done).await?;
    assert_eq!(done_items.len(), 1, "Should have 1 done item");
    
    println!("✅ Successfully retrieved items by status");
    
    Ok(())
}

/// Test updating processing item status
#[tokio::test]
#[serial]
async fn test_update_processing_item_status() -> Result<()> {
    println!("\n🧪 Running test: update_processing_item_status");
    
    // Get the shared test environment and AppState
    let mut env = TEST_ENV.lock().await;
    let app_state = env.get_app_state().await?;
    
    // Get the processing item repository
    let repo = app_state.persistency().processing_item_repository();
    
    // Create and store a new item
    let drive_item = create_test_file_item("file_to_update", "update_test.txt", None);
    let processing_item = create_test_processing_item(drive_item);
    let stored_id = repo.store_processing_item(&processing_item).await?;
    
    // Update the status
    repo.update_status_by_id(stored_id, &ProcessingStatus::Processing).await?;
    
    // Retrieve and verify
    let updated_item = repo.get_processing_item_by_id(stored_id).await?.unwrap();
    assert_eq!(updated_item.status, ProcessingStatus::Processing);
    
    // Update again
    repo.update_status_by_id(stored_id, &ProcessingStatus::Done).await?;
    
    // Retrieve and verify again
    let updated_item = repo.get_processing_item_by_id(stored_id).await?.unwrap();
    assert_eq!(updated_item.status, ProcessingStatus::Done);
    
    println!("✅ Successfully updated processing item status");
    
    Ok(())
}

/// Test to verify temp directories persist between tests
#[tokio::test]
#[serial]
async fn test_temp_directories_persistence() -> Result<()> {
    println!("\n🧪 Running test: temp_directories_persistence");
    
    // Get the shared test environment
    let env = TEST_ENV.lock().await;
    
    // Check that directories exist
    assert!(env.data_dir().exists(), "Data directory should exist");
    assert!(env.config_dir().exists(), "Config directory should exist");
    assert!(env.cache_dir().exists(), "Cache directory should exist");
    assert!(env.db_path().exists(), "Database file should exist");
    
    // Check OneDrive subdirectories
    let onedrive_data = env.data_dir().join("onedrive-sync");
    assert!(onedrive_data.join("downloads").exists(), "Downloads directory should exist");
    assert!(onedrive_data.join("uploads").exists(), "Uploads directory should exist");
    assert!(onedrive_data.join("local").exists(), "Local directory should exist");
    
    println!("✅ All test directories are persisted and accessible");
    println!("📁 Test environment root: {}", env.temp_dir_path().display());
    
    Ok(())
}

/// Cleanup test - should be run last to show final state
#[tokio::test]
#[serial]
async fn test_zzz_show_final_state() -> Result<()> {
    println!("\n🧪 Running test: show_final_state (cleanup)");
    
    // Get the shared test environment and AppState
    let mut env = TEST_ENV.lock().await;
    let app_state = env.get_app_state().await?;
    
    // Get the processing item repository
    let repo = app_state.persistency().processing_item_repository();
    
    // Show all items in the database
    let all_items = repo.get_all_processing_items().await?;
    println!("\n📊 Final database state:");
    println!("Total items: {}", all_items.len());
    
    for item in &all_items {
        println!(
            "  - ID: {:?}, OneDrive ID: {}, Name: {:?}, Status: {:?}",
            item.id,
            item.drive_item.id,
            item.drive_item.name,
            item.status
        );
    }
    
    println!("\n📁 Test environment location: {}", env.temp_dir_path().display());
    println!("   (This directory will persist until manually deleted)");
    
    // Optionally clear data for next test run
    // env.clear_all_data().await?;
    
    Ok(())
}