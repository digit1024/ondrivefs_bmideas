use anyhow::{anyhow, Result};
use async_trait::async_trait;
use onedrive_sync_daemon::onedrive_service::onedrive_client::OneDriveClientTrait;
use onedrive_sync_daemon::onedrive_service::onedrive_models::{
    CreateFolderResult, DeleteResult, DownloadResult, DriveItem, DeltaResponseApi, FileFacet,
    UploadResult, UploadSessionConfig, UserProfile,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Mock responses for various OneDrive operations
#[derive(Debug, Clone)]
pub struct MockResponses {
    pub user_profile: Option<UserProfile>,
    pub drive_items: HashMap<String, DriveItem>,
    pub delta_collections: Vec<DeltaResponseApi>,
    pub upload_results: Vec<UploadResult>,
    #[allow(dead_code)]
    pub create_folder_results: Vec<CreateFolderResult>,     
    #[allow(dead_code)]
    pub delete_results: Vec<DeleteResult>,
    #[allow(dead_code)]
    pub download_results: HashMap<String, DownloadResult>,
    #[allow(dead_code)]
    pub thumbnail_data: Vec<u8>,
    #[allow(dead_code)]
    pub should_fail_operations: Vec<String>, // List of operation names that should fail
}

impl Default for MockResponses {
    fn default() -> Self {
        Self {
            user_profile: Some(UserProfile {
                id: "mock_user_id".to_string(),
                display_name: Some("Mock User".to_string()),
                given_name: Some("Mock".to_string()),
                surname: Some("User".to_string()),
                mail: Some("mock@example.com".to_string()),
                user_principal_name: Some("mock@example.com".to_string()),
                job_title: Some("Test User".to_string()),
                business_phones: Some(vec!["123-456-7890".to_string()]),
                mobile_phone: Some("098-765-4321".to_string()),
                office_location: Some("Mock Office".to_string()),
                preferred_language: Some("en-US".to_string()),
            }),
            drive_items: HashMap::new(),
            delta_collections: vec![DeltaResponseApi {
                value: vec![],
                next_link: None,
                delta_link: Some("mock_delta_link".to_string()),
            }],
            upload_results: vec![UploadResult {
                onedrive_id: "mock_id".to_string(),
                etag: Some("mock_etag".to_string()),
                web_url: Some("mock_url".to_string()),
                size: Some(100),
            }],
            create_folder_results: vec![CreateFolderResult {
                onedrive_id: "mock_folder_id".to_string(),
                folder_name: "mock_folder".to_string(),
                web_url: Some("mock_folder_url".to_string()),
            }],
            delete_results: vec![DeleteResult {
                success: true,
                item_id: "mock_id".to_string(),
                item_path: "mock_path".to_string(),
            }],
            download_results: HashMap::new(),
            thumbnail_data: vec![0, 1, 2, 3, 4],
            should_fail_operations: vec![],
        }
    }
}

/// Mock implementation of OneDriveClientTrait for testing
#[derive(Clone)]
pub struct MockOneDriveClient {
    responses: Arc<Mutex<MockResponses>>,
    call_counter: Arc<Mutex<HashMap<String, usize>>>,
}

impl MockOneDriveClient {
    pub fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(MockResponses::default())),
            call_counter: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_failure() -> Self {
        let mut responses = MockResponses::default();
        responses.should_fail_operations = vec![
            "get_user_profile".to_string(),
            "get_item_by_id".to_string(),
            "upload".to_string(),
            "download".to_string(),
            "delete".to_string(),
            "create_folder".to_string(),
            "move_item".to_string(),
            "rename_item".to_string(),
            "get_delta_changes".to_string(),
        ];
        Self {
            responses: Arc::new(Mutex::new(responses)),
            call_counter: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Set expected user profile response
    pub fn set_expected_user_profile(&self, profile: UserProfile) {
        let mut responses = self.responses.lock().unwrap();
        responses.user_profile = Some(profile);
    }

    /// Set expected drive item for a specific ID
    pub fn set_expected_drive_item(&self, item_id: String, item: DriveItem) {
        let mut responses = self.responses.lock().unwrap();
        responses.drive_items.insert(item_id, item);
    }

    /// Set expected upload result
    pub fn set_expected_upload_result(&self, result: UploadResult) {
        let mut responses = self.responses.lock().unwrap();
        responses.upload_results.clear();
        responses.upload_results.push(result);
    }

    /// Set expected download result for a specific file
    #[allow(dead_code)]
    pub fn set_expected_download_result(&self, file_id: String, result: DownloadResult) {
        let mut responses = self.responses.lock().unwrap();
        responses.download_results.insert(file_id, result);
    }

    /// Set expected delta changes response
    #[allow(dead_code)]
    pub fn set_expected_delta_changes(&self, collection: DeltaResponseApi) {
        let mut responses = self.responses.lock().unwrap();
        responses.delta_collections.clear();
        responses.delta_collections.push(collection);
    }

    /// Set expected create folder result
    #[allow(dead_code)]
    pub fn set_expected_create_folder_result(&self, result: CreateFolderResult) {
        let mut responses = self.responses.lock().unwrap();
        responses.create_folder_results.clear();
        responses.create_folder_results.push(result);
    }

    /// Set expected thumbnail data
    #[allow(dead_code)]
    pub fn set_expected_thumbnail_data(&self, data: Vec<u8>) {
        let mut responses = self.responses.lock().unwrap();
        responses.thumbnail_data = data;
    }

    /// Make specific operations fail
    pub fn make_operation_fail(&self, operation: &str) {
        let mut responses = self.responses.lock().unwrap();
        if !responses.should_fail_operations.contains(&operation.to_string()) {
            responses.should_fail_operations.push(operation.to_string());
        }
    }

    /// Make all operations succeed (clear failure list)
    pub fn clear_operation_failures(&self) {
        let mut responses = self.responses.lock().unwrap();
        responses.should_fail_operations.clear();
    }

    /// Get call count for a specific operation
    pub fn get_call_count(&self, operation: &str) -> usize {
        let counter = self.call_counter.lock().unwrap();
        counter.get(operation).copied().unwrap_or(0)
    }

    /// Get all call counts
    pub fn get_all_call_counts(&self) -> HashMap<String, usize> {
        self.call_counter.lock().unwrap().clone()
    }

    /// Reset call counters
    pub fn reset_call_counters(&self) {
        let mut counter = self.call_counter.lock().unwrap();
        counter.clear();
    }

    /// Internal helper to increment call counter and check if operation should fail
    fn should_fail_operation(&self, operation: &str) -> bool {
        // Increment call counter
        {
            let mut counter = self.call_counter.lock().unwrap();
            *counter.entry(operation.to_string()).or_insert(0) += 1;
        }

        // Check if operation should fail
        let responses = self.responses.lock().unwrap();
        responses.should_fail_operations.contains(&operation.to_string())
    }
}

#[async_trait]
impl OneDriveClientTrait for MockOneDriveClient {
    async fn upload_large_file_to_parent(
        &self,
        _file_data: &[u8],
        _file_name: &str,
        _parent_id: &str,
        _config: Option<UploadSessionConfig>,
    ) -> Result<UploadResult> {
        if self.should_fail_operation("upload") {
            Err(anyhow!("Mock upload failure"))
        } else {
            let responses = self.responses.lock().unwrap();
            Ok(responses.upload_results.first()
                .cloned()
                .unwrap_or_else(|| UploadResult {
                    onedrive_id: "mock_id".to_string(),
                    etag: Some("mock_etag".to_string()),
                    web_url: Some("mock_url".to_string()),
                    size: Some(100),
                }))
        }
    }

    async fn update_large_file(
        &self,
        _file_data: &[u8],
        _item_id: &str,
        _config: Option<UploadSessionConfig>,
    ) -> Result<UploadResult> {
        if self.should_fail_operation("upload") {
            Err(anyhow!("Mock update failure"))
        } else {
            Ok(UploadResult {
                onedrive_id: "mock_id".to_string(),
                etag: Some("mock_etag".to_string()),
                web_url: Some("mock_url".to_string()),
                size: Some(100),
            })
        }
    }

    async fn upload_file_smart(
        &self,
        _file_data: &[u8],
        _file_name: &str,
        _parent_id: &str,
    ) -> Result<UploadResult> {
        if self.should_fail_operation("operation") {
            Err(anyhow!("Mock smart upload failure"))
        } else {
            Ok(UploadResult {
                onedrive_id: "mock_id".to_string(),
                etag: Some("mock_etag".to_string()),
                web_url: Some("mock_url".to_string()),
                size: Some(100),
            })
        }
    }

    async fn update_file_smart(&self, _file_data: &[u8], _item_id: &str) -> Result<UploadResult> {
        if self.should_fail_operation("operation") {
            Err(anyhow!("Mock smart update failure"))
        } else {
            Ok(UploadResult {
                onedrive_id: "mock_id".to_string(),
                etag: Some("mock_etag".to_string()),
                web_url: Some("mock_url".to_string()),
                size: Some(100),
            })
        }
    }

    async fn resume_large_file_upload(
        &self,
        _upload_url: &str,
        _file_data: &[u8],
        _config: Option<UploadSessionConfig>,
    ) -> Result<DriveItem> {
        if self.should_fail_operation("operation") {
            Err(anyhow!("Mock resume upload failure"))
        } else {
            Ok(DriveItem {
                id: "mock_id".to_string(),
                name: Some("mock_file".to_string()),
                etag: Some("mock_etag".to_string()),
                last_modified: Some("2023-01-01T00:00:00Z".to_string()),
                created_date: Some("2023-01-01T00:00:00Z".to_string()),
                size: Some(100),
                folder: None,
                file: Some(FileFacet {
                    mime_type: Some("text/plain".to_string()),
                }),
                download_url: Some("mock_download_url".to_string()),
                deleted: None,
                parent_reference: None,
            })
        }
    }

    async fn upload_new_file_to_parent(
        &self,
        _file_data: &[u8],
        _file_name: &str,
        _parent_id: &str,
    ) -> Result<UploadResult> {
        if self.should_fail_operation("operation") {
            Err(anyhow!("Mock new upload failure"))
        } else {
            Ok(UploadResult {
                onedrive_id: "mock_id".to_string(),
                etag: Some("mock_etag".to_string()),
                web_url: Some("mock_url".to_string()),
                size: Some(100),
            })
        }
    }

    async fn upload_updated_file(&self, _file_data: &[u8], _item_id: &str) -> Result<UploadResult> {
        if self.should_fail_operation("operation") {
            Err(anyhow!("Mock update upload failure"))
        } else {
            Ok(UploadResult {
                onedrive_id: "mock_id".to_string(),
                etag: Some("mock_etag".to_string()),
                web_url: Some("mock_url".to_string()),
                size: Some(100),
            })
        }
    }

    async fn get_item_by_id(&self, item_id: &str) -> Result<DriveItem> {
        if self.should_fail_operation("get_item_by_id") {
            Err(anyhow!("Mock get item failure"))
        } else {
            let responses = self.responses.lock().unwrap();
            Ok(responses.drive_items.get(item_id)
                .cloned()
                .unwrap_or_else(|| DriveItem {
                    id: item_id.to_string(),
                    name: Some("mock_file".to_string()),
                    etag: Some("mock_etag".to_string()),
                    last_modified: Some("2023-01-01T00:00:00Z".to_string()),
                    created_date: Some("2023-01-01T00:00:00Z".to_string()),
                    size: Some(100),
                    folder: None,
                    file: Some(FileFacet {
                        mime_type: Some("text/plain".to_string()),
                    }),
                    download_url: Some("mock_download_url".to_string()),
                    deleted: None,
                    parent_reference: None,
                }))
        }
    }

    async fn delete_item(&self, path: &str) -> Result<DeleteResult> {
        if self.should_fail_operation("operation") {
            Err(anyhow!("Mock delete failure"))
        } else {
            Ok(DeleteResult {
                success: true,
                item_id: "mock_id".to_string(),
                item_path: path.to_string(),
            })
        }
    }

    async fn create_folder(&self, _parent_path: &str, folder_name: &str) -> Result<CreateFolderResult> {
        if self.should_fail_operation("operation") {
            Err(anyhow!("Mock create folder failure"))
        } else {
            Ok(CreateFolderResult {
                onedrive_id: "mock_folder_id".to_string(),
                folder_name: folder_name.to_string(),
                web_url: Some("mock_folder_url".to_string()),
            })
        }
    }

    async fn move_item(&self, item_id: &str, _new_parent_id: &str) -> Result<DriveItem> {
        if self.should_fail_operation("operation") {
            Err(anyhow!("Mock move failure"))
        } else {
            Ok(DriveItem {
                id: item_id.to_string(),
                name: Some("moved_file".to_string()),
                etag: Some("mock_etag".to_string()),
                last_modified: Some("2023-01-01T00:00:00Z".to_string()),
                created_date: Some("2023-01-01T00:00:00Z".to_string()),
                size: Some(100),
                folder: None,
                file: Some(FileFacet {
                    mime_type: Some("text/plain".to_string()),
                }),
                download_url: Some("mock_download_url".to_string()),
                deleted: None,
                parent_reference: None,
            })
        }
    }

    async fn rename_item(&self, item_id: &str, new_name: &str) -> Result<DriveItem> {
        if self.should_fail_operation("operation") {
            Err(anyhow!("Mock rename failure"))
        } else {
            Ok(DriveItem {
                id: item_id.to_string(),
                name: Some(new_name.to_string()),
                etag: Some("mock_etag".to_string()),
                last_modified: Some("2023-01-01T00:00:00Z".to_string()),
                created_date: Some("2023-01-01T00:00:00Z".to_string()),
                size: Some(100),
                folder: None,
                file: Some(FileFacet {
                    mime_type: Some("text/plain".to_string()),
                }),
                download_url: Some("mock_download_url".to_string()),
                deleted: None,
                parent_reference: None,
            })
        }
    }

    async fn get_delta_changes(&self, _delta_token: Option<&str>) -> Result<DeltaResponseApi> {
        if self.should_fail_operation("get_delta_changes") {
            Err(anyhow!("Mock delta changes failure"))
        } else {
            let responses = self.responses.lock().unwrap();
            Ok(responses.delta_collections.first()
                .cloned()
                .unwrap_or_else(|| DeltaResponseApi {
                    value: vec![],
                    next_link: None,
                    delta_link: Some("mock_delta_link".to_string()),
                }))
        }
    }

    async fn download_thumbnail_medium(&self, _item_id: &str) -> Result<Vec<u8>> {
        if self.should_fail_operation("operation") {
            Err(anyhow!("Mock thumbnail download failure"))
        } else {
            Ok(vec![0, 1, 2, 3, 4]) // Mock thumbnail data
        }
    }

    async fn download_file_with_options(
        &self,
        _download_url: &str,
        item_id: &str,
        filename: &str,
        _range: Option<(u64, u64)>,
    ) -> Result<DownloadResult> {
        if self.should_fail_operation("operation") {
            Err(anyhow!("Mock download failure"))
        } else {
            Ok(DownloadResult {
                file_data: b"mock file content".to_vec(),
                file_name: filename.to_string(),
                onedrive_id: item_id.to_string(),
                etag: Some("mock_etag".to_string()),
                mime_type: Some("text/plain".to_string()),
                size: Some(17),
                last_modified: Some("2023-01-01T00:00:00Z".to_string()),
            })
        }
    }

    async fn download_file(
        &self,
        download_url: &str,
        item_id: &str,
        filename: &str,
    ) -> Result<DownloadResult> {
        self.download_file_with_options(download_url, item_id, filename, None).await
    }

    async fn get_user_profile(&self) -> Result<UserProfile> {
        if self.should_fail_operation("get_user_profile") {
            Err(anyhow!("Mock user profile failure"))
        } else {
            let responses = self.responses.lock().unwrap();
            Ok(responses.user_profile.clone().unwrap_or_else(|| UserProfile {
                id: "mock_user_id".to_string(),
                display_name: Some("Mock User".to_string()),
                given_name: Some("Mock".to_string()),
                surname: Some("User".to_string()),
                mail: Some("mock@example.com".to_string()),
                user_principal_name: Some("mock@example.com".to_string()),
                job_title: Some("Test User".to_string()),
                business_phones: Some(vec!["123-456-7890".to_string()]),
                mobile_phone: Some("098-765-4321".to_string()),
                office_location: Some("Mock Office".to_string()),
                preferred_language: Some("en-US".to_string()),
            }))
        }
    }

    async fn test_resumable_upload(&self) -> Result<()> {
        if self.should_fail_operation("operation") {
            Err(anyhow!("Mock test resumable upload failure"))
        } else {
            Ok(())
        }
    }
}
