use crate::auth::onedrive_auth::OneDriveAuth;
use crate::onedrive_service::http_client::HttpClient;
use crate::onedrive_service::onedrive_models::{
    CreateFolderResult, DeleteResult, DownloadResult, DriveItem, DeltaResponseApi, FileChunk,
    UploadProgress, UploadResult, UploadSessionConfig, UploadSessionItem, UploadSessionRequest,
    UploadSessionResponse, UploadSessionStatus, UserProfile,
};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use log::{debug, error, info, warn};
use serde_json;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use urlencoding;

/// Trait defining the interface for OneDrive client operations
#[async_trait]
pub trait OneDriveClientTrait: Send + Sync {
    // File upload operations
    async fn upload_large_file_to_parent(
        &self,
        file_data: &[u8],
        file_name: &str,
        parent_id: &str,
        config: Option<UploadSessionConfig>,
    ) -> Result<UploadResult>;

    async fn update_large_file(
        &self,
        file_data: &[u8],
        item_id: &str,
        config: Option<UploadSessionConfig>,
    ) -> Result<UploadResult>;

    async fn upload_file_smart(
        &self,
        file_data: &[u8],
        file_name: &str,
        parent_id: &str,
    ) -> Result<UploadResult>;

    async fn update_file_smart(&self, file_data: &[u8], item_id: &str) -> Result<UploadResult>;

    async fn resume_large_file_upload(
        &self,
        upload_url: &str,
        file_data: &[u8],
        config: Option<UploadSessionConfig>,
    ) -> Result<DriveItem>;

    async fn upload_new_file_to_parent(
        &self,
        file_data: &[u8],
        file_name: &str,
        parent_id: &str,
    ) -> Result<UploadResult>;

    async fn upload_updated_file(&self, file_data: &[u8], item_id: &str) -> Result<UploadResult>;

    // File operations
    async fn get_item_by_id(&self, item_id: &str) -> Result<DriveItem>;
    async fn delete_item(&self, path: &str) -> Result<DeleteResult>;
    async fn create_folder(&self, parent_path: &str, folder_name: &str) -> Result<CreateFolderResult>;
    async fn move_item(&self, item_id: &str, new_parent_id: &str) -> Result<DriveItem>;
    async fn rename_item(&self, item_id: &str, new_name: &str) -> Result<DriveItem>;

    // Delta synchronization
    async fn get_delta_changes(&self, delta_token: Option<&str>) -> Result<DeltaResponseApi>;

    // Download operations
    async fn download_thumbnail_medium(&self, item_id: &str) -> Result<Vec<u8>>;
    async fn download_file_with_options(
        &self,
        download_url: &str,
        item_id: &str,
        filename: &str,
        range: Option<(u64, u64)>,
    ) -> Result<DownloadResult>;
    async fn download_file(
        &self,
        download_url: &str,
        item_id: &str,
        filename: &str,
    ) -> Result<DownloadResult>;

    // User profile
    async fn get_user_profile(&self) -> Result<UserProfile>;

    // Test operations
    async fn test_resumable_upload(&self) -> Result<()>;
}

/// OneDrive API client that handles API operations
/// File system operations are handled by the FileManager trait
pub struct OneDriveClient {
    http_client: HttpClient,
    auth: Arc<OneDriveAuth>,
}
#[allow(dead_code)]
impl OneDriveClient {
    pub fn new(auth: Arc<OneDriveAuth>) -> Result<Self> {
        Ok(Self {
            http_client: HttpClient::new(),
            auth,
        })
    }

    /// Get authorization header with valid token
    async fn auth_header(&self) -> Result<String> {
        let token = self
            .auth
            .get_valid_token()
            .await
            .context("Failed to get valid token")?;
        //TODO: remove tis from debug
        debug!("Auth header: {}", token);
        Ok(format!("Bearer {}", token))
    }

    /// Create an upload session for large files
    async fn create_upload_session(
        &self,
        parent_id: &str,
        file_name: &str,
    ) -> Result<UploadSessionResponse> {
        let auth_header = self.auth_header().await?;
        let url = format!(
            "/me/drive/items/{}:/{}:/createUploadSession",
            parent_id, file_name
        );

        let request_body = UploadSessionRequest {
            item: UploadSessionItem {
                conflict_behavior: "rename".to_string(),
                name: file_name.to_string(),
            },
        };

        info!(
            "Creating upload session for file: {} in parent: {}",
            file_name, parent_id
        );

        let session = self
            .http_client
            .create_upload_session(&url, &request_body, &auth_header)
            .await?;

        info!("Created upload session: {}", session.upload_url);
        Ok(session)
    }

    /// Create an upload session for updating existing files
    async fn create_update_upload_session(&self, item_id: &str) -> Result<UploadSessionResponse> {
        let auth_header = self.auth_header().await?;
        let url = format!("/me/drive/items/{}/createUploadSession", item_id);

        let request_body = UploadSessionRequest {
            item: UploadSessionItem {
                conflict_behavior: "replace".to_string(),
                name: "".to_string(), // Not needed for updates
            },
        };

        info!("Creating update upload session for item: {}", item_id);

        let session = self
            .http_client
            .create_upload_session(&url, &request_body, &auth_header)
            .await?;

        info!("Created update upload session: {}", session.upload_url);
        Ok(session)
    }

    /// Split file data into chunks
    fn split_file_into_chunks(&self, file_data: &[u8], chunk_size: u64) -> Vec<FileChunk> {
        let mut chunks = Vec::new();
        let total_size = file_data.len() as u64;

        let mut start = 0;
        while start < total_size {
            let end = std::cmp::min(start + chunk_size - 1, total_size - 1);
            let chunk_data = file_data[start as usize..=end as usize].to_vec();

            chunks.push(FileChunk {
                start,
                end,
                data: chunk_data,
            });

            start = end + 1;
        }

        chunks
    }

    /// Upload a single chunk with retry logic
    async fn upload_chunk_with_retry(
        &self,
        upload_url: &str,
        chunk: &FileChunk,
        total_size: u64,
        config: &UploadSessionConfig,
    ) -> Result<reqwest::Response> {
        let content_range = format!("bytes {}-{}/{}", chunk.start, chunk.end, total_size);

        for attempt in 0..=config.max_retries {
            match self
                .http_client
                .upload_file_chunk(upload_url, &chunk.data, &content_range)
                .await
            {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() || status.as_u16() == 202 {
                        return Ok(response);
                    } else {
                        let error_text = response.text().await.unwrap_or_default();
                        warn!("Upload chunk failed with status {}: {}", status, error_text);
                    }
                }
                Err(e) => {
                    warn!("Upload chunk attempt {} failed: {}", attempt + 1, e);
                }
            }

            if attempt < config.max_retries {
                let delay = config.retry_delay_ms * (1 << attempt) as u64;
                sleep(Duration::from_millis(delay)).await;
            }
        }

        Err(anyhow!(
            "Failed to upload chunk after {} attempts",
            config.max_retries + 1
        ))
    }

    /// Upload large file using resumable upload session
    async fn upload_large_file(
        &self,
        upload_url: &str,
        file_data: &[u8],
        config: Option<UploadSessionConfig>,
    ) -> Result<DriveItem> {
        let config = config.unwrap_or_default();
        let total_size = file_data.len() as u64;

        // Ensure chunk size is a multiple of 320KB as required by Microsoft
        let adjusted_chunk_size = (config.chunk_size / 327680) * 327680;
        if adjusted_chunk_size != config.chunk_size {
            warn!(
                "Adjusted chunk size from {} to {} to meet 320KB requirement",
                config.chunk_size, adjusted_chunk_size
            );
        }

        let chunks = self.split_file_into_chunks(file_data, adjusted_chunk_size);
        info!(
            "Split file into {} chunks of {} bytes each",
            chunks.len(),
            adjusted_chunk_size
        );

        let mut completed_chunks = 0;
        let mut final_response: Option<reqwest::Response> = None;

        for (index, chunk) in chunks.iter().enumerate() {
            let response = self
                .upload_chunk_with_retry(upload_url, chunk, total_size, &config)
                .await?;
            completed_chunks += 1;

            let progress = (completed_chunks as f64 / chunks.len() as f64) * 100.0;
            info!(
                "Upload progress: {:.1}% ({}/{})",
                progress,
                completed_chunks,
                chunks.len()
            );

            // Store the final response (last chunk)
            if index == chunks.len() - 1 {
                final_response = Some(response);
            }
        }

        // Parse the final response to get the DriveItem
        if let Some(response) = final_response {
            let status = response.status();
            if status.is_success() || status.as_u16() == 201 {
                let drive_item: DriveItem = response
                    .json()
                    .await
                    .context("Failed to parse final upload response")?;
                return Ok(drive_item);
            } else {
                let error_text = response.text().await.unwrap_or_default();
                return Err(anyhow!(
                    "Final upload failed with status {}: {}",
                    status,
                    error_text
                ));
            }
        }

        // Fallback: create a basic DriveItem if we can't parse the response
        warn!("Could not parse final upload response, creating basic DriveItem");
        Ok(DriveItem {
            id: "".to_string(),
            name: None,
            etag: None,
            last_modified: None,
            created_date: None,
            size: Some(total_size),
            folder: None,
            file: None,
            download_url: None,
            deleted: None,
            parent_reference: None,
        })
    }

    /// Upload large file to parent folder using resumable upload
    pub async fn upload_large_file_to_parent(
        &self,
        file_data: &[u8],
        file_name: &str,
        parent_id: &str,
        config: Option<UploadSessionConfig>,
    ) -> Result<UploadResult> {
        info!(
            "Starting large file upload: {} to parent {}",
            file_name, parent_id
        );

        let session = self.create_upload_session(parent_id, file_name).await?;

        let drive_item = self
            .upload_large_file(&session.upload_url, file_data, config)
            .await?;

        let result = UploadResult {
            onedrive_id: drive_item.id.clone(),
            etag: drive_item.etag,
            web_url: None,
            size: drive_item.size,
        };

        info!(
            "Completed large file upload: {} -> {}",
            file_name, drive_item.id
        );
        Ok(result)
    }

    /// Update large existing file using resumable upload
    pub async fn update_large_file(
        &self,
        file_data: &[u8],
        item_id: &str,
        config: Option<UploadSessionConfig>,
    ) -> Result<UploadResult> {
        info!("Starting large file update: {}", item_id);

        let session = self.create_update_upload_session(item_id).await?;

        let drive_item = self
            .upload_large_file(&session.upload_url, file_data, config)
            .await?;

        let result = UploadResult {
            onedrive_id: drive_item.id.clone(),
            etag: drive_item.etag,
            web_url: None,
            size: drive_item.size,
        };

        info!(
            "Completed large file update: {} -> {}",
            item_id, drive_item.id
        );
        Ok(result)
    }

    /// Smart upload that automatically chooses between simple and resumable upload
    pub async fn upload_file_smart(
        &self,
        file_data: &[u8],
        file_name: &str,
        parent_id: &str,
    ) -> Result<UploadResult> {
        const LARGE_FILE_THRESHOLD: usize = 4 * 1024 * 1024; // 4MB

        if file_data.len() > LARGE_FILE_THRESHOLD {
            info!(
                "File size {} bytes exceeds {} bytes, using resumable upload",
                file_data.len(),
                LARGE_FILE_THRESHOLD
            );
            self.upload_large_file_to_parent(file_data, file_name, parent_id, None)
                .await
        } else {
            info!(
                "File size {} bytes is under {} bytes, using simple upload",
                file_data.len(),
                LARGE_FILE_THRESHOLD
            );
            self.upload_new_file_to_parent(file_data, file_name, parent_id)
                .await
        }
    }

    /// Smart update that automatically chooses between simple and resumable upload
    pub async fn update_file_smart(&self, file_data: &[u8], item_id: &str) -> Result<UploadResult> {
        const LARGE_FILE_THRESHOLD: usize = 4 * 1024 * 1024; // 4MB

        if file_data.len() > LARGE_FILE_THRESHOLD {
            info!(
                "File size {} bytes exceeds {} bytes, using resumable update",
                file_data.len(),
                LARGE_FILE_THRESHOLD
            );
            self.update_large_file(file_data, item_id, None).await
        } else {
            info!(
                "File size {} bytes is under {} bytes, using simple update",
                file_data.len(),
                LARGE_FILE_THRESHOLD
            );
            self.upload_updated_file(file_data, item_id).await
        }
    }

    /// Resume an interrupted upload by checking session status and uploading missing chunks
    pub async fn resume_large_file_upload(
        &self,
        upload_url: &str,
        file_data: &[u8],
        config: Option<UploadSessionConfig>,
    ) -> Result<DriveItem> {
        info!("Attempting to resume upload at: {}", upload_url);

        // Get current session status
        let status = self
            .http_client
            .get_upload_session_status(upload_url)
            .await?;

        info!("Upload session status: {:?}", status);

        let config = config.unwrap_or_default();
        let total_size = file_data.len() as u64;

        // Parse next expected ranges to determine what's missing
        let mut missing_ranges = Vec::new();
        for range_str in &status.next_expected_ranges {
            if let Some((start, end)) = self.parse_range_string(range_str) {
                missing_ranges.push((start, end));
            }
        }

        if missing_ranges.is_empty() {
            info!("No missing ranges found, upload appears to be complete");
            // Try to get the final result
            return self.get_final_upload_result(upload_url).await;
        }

        info!("Found {} missing ranges to upload", missing_ranges.len());

        // Upload missing chunks
        for (start, end) in missing_ranges {
            let chunk_data = file_data[start as usize..=end as usize].to_vec();
            let chunk = FileChunk {
                start,
                end,
                data: chunk_data,
            };

            self.upload_chunk_with_retry(upload_url, &chunk, total_size, &config)
                .await?;
            info!("Uploaded missing chunk: bytes {}-{}", start, end);
        }

        // Get final result
        self.get_final_upload_result(upload_url).await
    }

    /// Parse range string like "12345-" or "12345-55232"
    fn parse_range_string(&self, range_str: &str) -> Option<(u64, u64)> {
        let parts: Vec<&str> = range_str.split('-').collect();
        if parts.len() != 2 {
            return None;
        }

        let start = parts[0].parse::<u64>().ok()?;
        let end = if parts[1].is_empty() {
            // Open range like "12345-", use a reasonable default
            start + 10 * 1024 * 1024 - 1 // 10MB chunk
        } else {
            parts[1].parse::<u64>().ok()?
        };

        Some((start, end))
    }

    /// Get the final upload result after all chunks are uploaded
    async fn get_final_upload_result(&self, upload_url: &str) -> Result<DriveItem> {
        // The final response should contain the DriveItem
        // We'll try to get it from the session status first
        let status = self
            .http_client
            .get_upload_session_status(upload_url)
            .await?;

        if status.next_expected_ranges.is_empty() {
            // Upload is complete, but we need to get the DriveItem
            // This is a limitation - we need to store the DriveItem from the final chunk response
            warn!("Upload appears complete but DriveItem not available from status");
        }

        // For now, return a placeholder - in a real implementation,
        // we would store the DriveItem from the final chunk response
        Ok(DriveItem {
            id: "".to_string(),
            name: None,
            etag: None,
            last_modified: None,
            created_date: None,
            size: None,
            folder: None,
            file: None,
            download_url: None,
            deleted: None,
            parent_reference: None,
        })
    }

    #[cfg(test)]
    /// Test function to verify resumable upload functionality
    pub async fn test_resumable_upload(&self) -> Result<()> {
        // Create a test file larger than 4MB
        let test_data = vec![0u8; 5 * 1024 * 1024]; // 5MB file
        let test_filename = "test_large_file.bin";
        let test_parent_id = "root"; // Use root as parent

        info!("Testing resumable upload with {} bytes", test_data.len());

        // Test smart upload (should automatically choose resumable)
        let result = self
            .upload_file_smart(&test_data, test_filename, test_parent_id)
            .await?;

        info!("Smart upload test completed: {:?}", result);
        Ok(())
    }

    /// Upload a file to a specific parent folder by parent ID (correct Microsoft Graph API format)
    pub async fn upload_new_file_to_parent(
        &self,
        file_data: &[u8],
        file_name: &str,
        parent_id: &str,
    ) -> Result<UploadResult> {
        let auth_header = self.auth_header().await?;
        let upload_url = format!("/me/drive/items/{}:/{}:/content", parent_id, file_name);
        info!(
            "Uploading file: {} to parent {} using URL: {}",
            file_name, parent_id, upload_url
        );

        let response = self
            .http_client
            .upload_file(&upload_url, file_data, &auth_header)
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Failed to upload file: {}", error_text));
        }

        let item: DriveItem = response.json().await?;

        let result = UploadResult {
            onedrive_id: item.id.clone(),
            etag: item.etag,
            web_url: None,
            size: item.size,
        };

        info!(
            "Uploaded file: {} to parent {} -> {}",
            file_name, parent_id, item.id
        );
        Ok(result)
    }

    /// Update an existing file on OneDrive and return the update result

    pub async fn upload_updated_file(
        &self,
        file_data: &[u8],
        item_id: &str,
    ) -> Result<UploadResult> {
        let auth_header = self.auth_header().await?;
        let url = format!("/me/drive/items/{}/content", item_id);

        let response = self
            .http_client
            .upload_file(&url, file_data, &auth_header)
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Failed to update file: {}", error_text));
        }

        let item: DriveItem = response.json().await?;

        let result = UploadResult {
            onedrive_id: item.id.clone(),
            etag: item.etag,
            web_url: None,
            size: item.size,
        };

        info!("Updated file: {} -> {}", item_id, item.id);
        Ok(result)
    }

    /// Get item by OneDrive ID
    pub async fn get_item_by_id(&self, item_id: &str) -> Result<DriveItem> {
        let auth_header = self.auth_header().await?;
        let url = format!("/me/drive/items/{}", item_id);

        let item: DriveItem = self
            .http_client
            .get(&url, &auth_header)
            .await
            .context("Failed to get item by ID")?;

        Ok(item)
    }

    /// Delete an item by path and return the delete result

    pub async fn delete_item(&self, path: &str) -> Result<DeleteResult> {
        let auth_header = self.auth_header().await?;

        // Strip /drive/root: prefix if present and encode the relative path
        let relative_path = if path.starts_with("/drive/root:") {
            &path[12..] // Remove "/drive/root:" prefix
        } else {
            path
        };

        let encoded_path = urlencoding::encode(relative_path);
        let url = format!("/me/drive/root:{}", encoded_path);

        self.http_client
            .delete(&url, &auth_header)
            .await
            .context("Failed to delete item")?;

        let result = DeleteResult {
            success: true,
            item_id: "".to_string(), // OneDrive API doesn't return item ID on delete
            item_path: path.to_string(),
        };

        info!("Deleted item: {}", path);
        Ok(result)
    }

    /// Create a folder and return the creation result

    pub async fn create_folder(
        &self,
        parent_path: &str,
        folder_name: &str,
    ) -> Result<CreateFolderResult> {
        let auth_header = self.auth_header().await?;
        let url = self.build_create_folder_url(parent_path)?;
        let body = self.build_create_folder_body(folder_name);

        let item: DriveItem = self
            .http_client
            .post(&url, &body, &auth_header)
            .await
            .context("Failed to create folder")?;

        let result = CreateFolderResult {
            onedrive_id: item.id,
            folder_name: folder_name.to_string(),
            web_url: None, // OneDrive API doesn't return web_url in this endpoint
        };

        info!("Created folder: {}", folder_name);
        Ok(result)
    }

    /// Move an item to a new parent folder

    pub async fn move_item(&self, item_id: &str, new_parent_id: &str) -> Result<DriveItem> {
        let auth_header = self.auth_header().await?;
        let url = format!("/me/drive/items/{}/move", item_id);
        let body = self.build_move_item_body(new_parent_id);

        let item: DriveItem = self
            .http_client
            .post(&url, &body, &auth_header)
            .await
            .context("Failed to move item")?;

        info!("Moved item: {} to parent: {}", item_id, new_parent_id);
        Ok(item)
    }

    /// Build create folder URL

    fn build_create_folder_url(&self, parent_path: &str) -> Result<String> {
        let url = if parent_path == "/" || parent_path == "/drive/root:" {
            "/me/drive/root/children".to_string()
        } else {
            // Strip /drive/root: prefix and encode the relative path
            let relative_path = if parent_path.starts_with("/drive/root:") {
                &parent_path[12..] // Remove "/drive/root:" prefix
            } else {
                parent_path
            };

            let encoded_path = urlencoding::encode(relative_path);
            format!("/me/drive/root:{}:/children", encoded_path)
        };

        Ok(url)
    }

    /// Build create folder request body

    fn build_create_folder_body(&self, folder_name: &str) -> serde_json::Value {
        serde_json::json!({
            "name": folder_name,
            "folder": {},
            "@microsoft.graph.conflictBehavior": "rename"
        })
    }

    /// Build move item request body

    fn build_move_item_body(&self, new_parent_id: &str) -> serde_json::Value {
        serde_json::json!({
            "parentReference": {
                "id": new_parent_id
            }
        })
    }

    /// Rename an item (change its name)

    pub async fn rename_item(&self, item_id: &str, new_name: &str) -> Result<DriveItem> {
        let auth_header = self.auth_header().await?;
        let url = format!("/me/drive/items/{}", item_id);
        let body = self.build_rename_item_body(new_name);

        // Send PATCH request to update the name
        self.http_client
            .patch::<serde_json::Value>(&url, &body, &auth_header)
            .await
            .context("Failed to rename item")?;

        // Get the updated item to return
        let updated_item = self.get_item_by_id(item_id).await?;

        info!("Renamed item: {} to: {}", item_id, new_name);
        Ok(updated_item)
    }

    /// Build rename item request body

    fn build_rename_item_body(&self, new_name: &str) -> serde_json::Value {
        serde_json::json!({
            "name": new_name
        })
    }

    /// Get delta changes for a folder using delta token

    pub async fn get_delta_changes(
        &self,

        delta_token: Option<&str>,
    ) -> Result<DeltaResponseApi> {
        let url = self.build_delta_url(delta_token);
        let auth_header = self.auth_header().await?;

        let collection: DeltaResponseApi = self
            .http_client
            .get(&url, &auth_header)
            .await
            .context("Failed to get delta changes")?;

        Ok(collection)
    }

    /// Build delta URL with optional token

    fn build_delta_url(&self, delta_token: Option<&str>) -> String {
        // it maay be full url or just token
        // if it starts with http lets return same

        if let Some(delta_token) = delta_token {
            if delta_token.starts_with("http") {
                return delta_token.to_string();
            }
            format!("/me/drive/root/delta?token={}", delta_token.to_string())
        } else {
            format!("/me/drive/root/delta")
        }
    }

    /// Download a medium thumbnail for an item
    pub async fn download_thumbnail_medium(&self, item_id: &str) -> Result<Vec<u8>> {
        let auth_header = self.auth_header().await?;
        let rel = format!("/me/drive/items/{}/thumbnails/0/medium/content", item_id);
        let url = self
            .http_client
            .get_full_url(&rel)
            .context("Failed to build thumbnail url")?;
        let response = self
            .http_client
            .request_builder("GET", &url)
            .header("Authorization", auth_header)
            .send()
            .await
            .context("Failed to send thumbnail request")?;
        if !response.status().is_success() {
            return Err(anyhow!("Thumbnail download failed with status: {}", response.status()));
        }
        let bytes = response.bytes().await.context("Failed to read thumbnail bytes")?;
        Ok(bytes.to_vec())
    }

    /// Download file with optional range and thumbnail support
    pub async fn download_file_with_options(
        &self,
        download_url: &str,
        item_id: &str,
        filename: &str,
        range: Option<(u64, u64)>, // (start, end) bytes
    ) -> Result<DownloadResult> {
        // Build the request using the request builder
        let mut request = self.http_client.request_builder("GET", download_url);

        // Add Range header if specified
        if let Some((start, end)) = range {
            let range_header = format!("bytes={}-{}", start, end);
            request = request.header("Range", range_header);
        }

        let response = request
            .send()
            .await
            .context("Failed to send download request")?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Download failed with status: {}",
                response.status()
            ));
        }

        // Extract headers before consuming the response
        let etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim_matches('"').to_string());

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let content_length = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());

        let last_modified = response
            .headers()
            .get("last-modified")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let file_data = response
            .bytes()
            .await
            .context("Failed to read response bytes")?;

        Ok(DownloadResult {
            file_data: file_data.to_vec(),
            file_name: filename.to_string(),
            onedrive_id: item_id.to_string(),
            etag,
            mime_type: content_type,
            size: content_length,
            last_modified,
        })
    }

    /// Download full file (existing method, updated to use new function)
    pub async fn download_file(
        &self,
        download_url: &str,
        item_id: &str,
        filename: &str,
    ) -> Result<DownloadResult> {
        self.download_file_with_options(
            download_url,
            item_id,
            filename,
            None, // no range = full download
        )
        .await
    }

    /// Get user profile information from Microsoft Graph API
    pub async fn get_user_profile(&self) -> Result<UserProfile> {
        let auth_header = self
            .auth_header()
            .await
            .context("Failed to get auth header")?;
        debug!("Auth header: {}", auth_header);
        let url = "/me";

        let profile: UserProfile = self
            .http_client
            .get(url, &auth_header)
            .await
            .context("Failed to get user profile")?;

        info!(
            "Retrieved user profile for: {}",
            profile.display_name.as_deref().unwrap_or("Unknown")
        );
        Ok(profile)
    }
}

#[async_trait]
impl OneDriveClientTrait for OneDriveClient {
    async fn upload_large_file_to_parent(
        &self,
        file_data: &[u8],
        file_name: &str,
        parent_id: &str,
        config: Option<UploadSessionConfig>,
    ) -> Result<UploadResult> {
        self.upload_large_file_to_parent(file_data, file_name, parent_id, config).await
    }

    async fn update_large_file(
        &self,
        file_data: &[u8],
        item_id: &str,
        config: Option<UploadSessionConfig>,
    ) -> Result<UploadResult> {
        self.update_large_file(file_data, item_id, config).await
    }

    async fn upload_file_smart(
        &self,
        file_data: &[u8],
        file_name: &str,
        parent_id: &str,
    ) -> Result<UploadResult> {
        self.upload_file_smart(file_data, file_name, parent_id).await
    }

    async fn update_file_smart(&self, file_data: &[u8], item_id: &str) -> Result<UploadResult> {
        self.update_file_smart(file_data, item_id).await
    }

    async fn resume_large_file_upload(
        &self,
        upload_url: &str,
        file_data: &[u8],
        config: Option<UploadSessionConfig>,
    ) -> Result<DriveItem> {
        self.resume_large_file_upload(upload_url, file_data, config).await
    }

    async fn upload_new_file_to_parent(
        &self,
        file_data: &[u8],
        file_name: &str,
        parent_id: &str,
    ) -> Result<UploadResult> {
        self.upload_new_file_to_parent(file_data, file_name, parent_id).await
    }

    async fn upload_updated_file(&self, file_data: &[u8], item_id: &str) -> Result<UploadResult> {
        self.upload_updated_file(file_data, item_id).await
    }

    async fn get_item_by_id(&self, item_id: &str) -> Result<DriveItem> {
        self.get_item_by_id(item_id).await
    }

    async fn delete_item(&self, path: &str) -> Result<DeleteResult> {
        self.delete_item(path).await
    }

    async fn create_folder(&self, parent_path: &str, folder_name: &str) -> Result<CreateFolderResult> {
        self.create_folder(parent_path, folder_name).await
    }

    async fn move_item(&self, item_id: &str, new_parent_id: &str) -> Result<DriveItem> {
        self.move_item(item_id, new_parent_id).await
    }

    async fn rename_item(&self, item_id: &str, new_name: &str) -> Result<DriveItem> {
        self.rename_item(item_id, new_name).await
    }

    async fn get_delta_changes(&self, delta_token: Option<&str>) -> Result<DeltaResponseApi> {
        self.get_delta_changes(delta_token).await
    }

    async fn download_thumbnail_medium(&self, item_id: &str) -> Result<Vec<u8>> {
        self.download_thumbnail_medium(item_id).await
    }

    async fn download_file_with_options(
        &self,
        download_url: &str,
        item_id: &str,
        filename: &str,
        range: Option<(u64, u64)>,
    ) -> Result<DownloadResult> {
        self.download_file_with_options(download_url, item_id, filename, range).await
    }

    async fn download_file(
        &self,
        download_url: &str,
        item_id: &str,
        filename: &str,
    ) -> Result<DownloadResult> {
        self.download_file(download_url, item_id, filename).await
    }

    async fn get_user_profile(&self) -> Result<UserProfile> {
        self.get_user_profile().await
    }

    async fn test_resumable_upload(&self) -> Result<()> {
        self.test_resumable_upload().await
    }
}

impl Clone for OneDriveClient {
    fn clone(&self) -> Self {
        Self {
            http_client: self.http_client.clone(),
            auth: self.auth.clone(),
        }
    }
}


