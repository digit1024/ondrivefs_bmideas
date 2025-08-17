use anyhow::Result;
use crate::common::setup::TEST_ENV;

#[tokio::test]
async fn test_app_state_with_mock() -> Result<()> {
    let mut env = TEST_ENV.lock().await;
    let app_state = env.get_app_state_with_mock().await?;
    drop(env);
    
    // Test that the mock OneDrive client is working
    let user_profile = app_state.onedrive().get_user_profile().await?;
    assert_eq!(user_profile.id, "mock_user_id");
    assert_eq!(user_profile.display_name, Some("Mock User".to_string()));
    
    // Test database operations still work
    let processing_repo = app_state.persistency().processing_item_repository();
    let items = processing_repo.get_all_processing_items().await?;
    assert!(items.is_empty()); // Should be empty in test
    
    println!("✅ Mock AppState test completed successfully");
    
    Ok(())
}
