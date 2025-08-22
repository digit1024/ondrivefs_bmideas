use anyhow::Result;
use onedrive_sync_daemon::onedrive_service::onedrive_client::OneDriveClientTrait;
use onedrive_sync_daemon::onedrive_service::onedrive_models::{UserProfile, DriveItem, FileFacet, DeltaResponseApi};
use crate::common::mock_onedrive_client::MockOneDriveClient;

#[tokio::test]
async fn test_injectable_user_profile() -> Result<()> {
    let mock_client = MockOneDriveClient::new();
    
    // Set a custom user profile
    let custom_profile = UserProfile {
        id: "custom_user_id".to_string(),
        display_name: Some("Custom Test User".to_string()),
        given_name: Some("Custom".to_string()),
        surname: Some("User".to_string()),
        mail: Some("custom@test.com".to_string()),
        user_principal_name: Some("custom@test.com".to_string()),
        job_title: Some("Test Engineer".to_string()),
        business_phones: Some(vec!["555-0123".to_string()]),
        mobile_phone: Some("555-0456".to_string()),
        office_location: Some("Test Office".to_string()),
        preferred_language: Some("en-US".to_string()),
    };
    
    mock_client.set_expected_user_profile(custom_profile.clone());
    
    // Test that we get the custom profile back
    let result = mock_client.get_user_profile().await?;
    assert_eq!(result.id, "custom_user_id");
    assert_eq!(result.display_name, Some("Custom Test User".to_string()));
    assert_eq!(result.job_title, Some("Test Engineer".to_string()));
    
    // Verify call count
    assert_eq!(mock_client.get_call_count("get_user_profile"), 1);
    
    Ok(())
}

#[tokio::test]
async fn test_injectable_drive_item() -> Result<()> {
    let mock_client = MockOneDriveClient::new();
    
    // Set a custom drive item for a specific ID
    let custom_item = DriveItem {
        id: "test_file_123".to_string(),
        name: Some("test_document.txt".to_string()),
        etag: Some("custom_etag_456".to_string()),
        last_modified: Some("2024-01-15T10:30:00Z".to_string()),
        created_date: Some("2024-01-15T10:00:00Z".to_string()),
        size: Some(2048),
        folder: None,
        file: Some(FileFacet {
            mime_type: Some("text/plain".to_string()),
        }),
        download_url: Some("https://custom.download.url".to_string()),
        deleted: None,
        parent_reference: None,
        ctag: None,
    };
    
    mock_client.set_expected_drive_item("test_file_123".to_string(), custom_item.clone());
    
    // Test that we get the custom item back
    let result = mock_client.get_item_by_id("test_file_123").await?;
    assert_eq!(result.id, "test_file_123");
    assert_eq!(result.name, Some("test_document.txt".to_string()));
    assert_eq!(result.etag, Some("custom_etag_456".to_string()));
    assert_eq!(result.size, Some(2048));
    
    // Test that a different ID still gets the default
    let default_result = mock_client.get_item_by_id("different_id").await?;
    assert_eq!(default_result.id, "different_id");
    assert_eq!(default_result.name, Some("mock_file".to_string())); // Default name
    
    // Verify call counts
    assert_eq!(mock_client.get_call_count("get_item_by_id"), 2);
    
    Ok(())
}

#[tokio::test]
async fn test_selective_operation_failures() -> Result<()> {
    let mock_client = MockOneDriveClient::new();
    
    // Make only user profile operations fail
    mock_client.make_operation_fail("get_user_profile");
    
    // User profile should fail
    let profile_result = mock_client.get_user_profile().await;
    assert!(profile_result.is_err());
    assert!(profile_result.unwrap_err().to_string().contains("Mock user profile failure"));
    
    // But drive item should still work
    let item_result = mock_client.get_item_by_id("test_id").await?;
    assert_eq!(item_result.id, "test_id");
    
    // Verify call counts
    assert_eq!(mock_client.get_call_count("get_user_profile"), 1);
    assert_eq!(mock_client.get_call_count("get_item_by_id"), 1);
    
    // Clear failures and try again
    mock_client.clear_operation_failures();
    let profile_result2 = mock_client.get_user_profile().await?;
    assert_eq!(profile_result2.id, "mock_user_id");
    
    Ok(())
}

#[tokio::test]
async fn test_call_counting() -> Result<()> {
    let mock_client = MockOneDriveClient::new();
    
    // Make multiple calls
    let _ = mock_client.get_user_profile().await?;
    let _ = mock_client.get_user_profile().await?;
    let _ = mock_client.get_item_by_id("test1").await?;
    let _ = mock_client.get_item_by_id("test2").await?;
    let _ = mock_client.get_item_by_id("test3").await?;
    
    // Check individual counts
    assert_eq!(mock_client.get_call_count("get_user_profile"), 2);
    assert_eq!(mock_client.get_call_count("get_item_by_id"), 3);
    assert_eq!(mock_client.get_call_count("get_delta_changes"), 0);
    
    // Check all counts
    let all_counts = mock_client.get_all_call_counts();
    assert_eq!(all_counts.get("get_user_profile"), Some(&2));
    assert_eq!(all_counts.get("get_item_by_id"), Some(&3));
    
    // Reset and verify
    mock_client.reset_call_counters();
    assert_eq!(mock_client.get_call_count("get_user_profile"), 0);
    assert_eq!(mock_client.get_call_count("get_item_by_id"), 0);
    
    Ok(())
}
