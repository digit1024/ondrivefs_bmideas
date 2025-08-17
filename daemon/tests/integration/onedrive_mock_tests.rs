use anyhow::Result;
use onedrive_sync_daemon::onedrive_service::onedrive_client::OneDriveClientTrait;
use crate::common::mock_onedrive_client::MockOneDriveClient;

#[tokio::test]
async fn test_mock_onedrive_client_success() -> Result<()> {
    let mock_client = MockOneDriveClient::new();
    
    // Test successful operations
    let user_profile = mock_client.get_user_profile().await?;
    assert_eq!(user_profile.id, "mock_user_id");
    assert_eq!(user_profile.display_name, Some("Mock User".to_string()));
    
    let drive_item = mock_client.get_item_by_id("test_id").await?;
    assert_eq!(drive_item.id, "test_id");
    assert_eq!(drive_item.name, Some("mock_file".to_string()));
    
    Ok(())
}

#[tokio::test]
async fn test_mock_onedrive_client_failure() -> Result<()> {
    let mock_client = MockOneDriveClient::with_failure();
    
    // Test failure operations
    let user_result = mock_client.get_user_profile().await;
    assert!(user_result.is_err());
    assert!(user_result.unwrap_err().to_string().contains("Mock user profile failure"));
    
    let item_result = mock_client.get_item_by_id("test_id").await;
    assert!(item_result.is_err());
    assert!(item_result.unwrap_err().to_string().contains("Mock get item failure"));
    
    Ok(())
}

#[tokio::test]
async fn test_delta_changes_mock() -> Result<()> {
    let mock_client = MockOneDriveClient::new();
    
    let delta_response = mock_client.get_delta_changes(None).await?;
    assert!(delta_response.value.is_empty());
    assert_eq!(delta_response.delta_link, Some("mock_delta_link".to_string()));
    
    Ok(())
}

