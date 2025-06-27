use anyhow::{anyhow, Result};
use reqwest::Client;
use urlencoding;
use log::info;
use crate::onedrive_service::onedrive_models::{
    DriveItem, 
    DriveItemCollection,
    DownloadResult,
    UploadResult,
    CreateFolderResult,
    DeleteResult
};
use crate::auth::onedrive_auth::OneDriveAuth;

const GRAPH_API_BASE: &str = "https://graph.microsoft.com/v1.0";

/// OneDrive API client that only handles API operations
/// File system operations are handled by the FileManager trait
pub struct OneDriveClient {
    client: Client,
    auth: OneDriveAuth,
}

impl OneDriveClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: Client::new(),
            auth: OneDriveAuth::new()?,
        })
    }

    /// Get authorization header with valid token
    async fn auth_header(&self) -> Result<String> {
        let token = self.auth.get_valid_token().await?;
        Ok(format!("Bearer {}", token))
    }

    /// List items in root directory
    pub async fn list_root(&self) -> Result<Vec<DriveItem>> {
        self.list_folder_by_url("/me/drive/root/children").await
    }

    /// List items in a specific folder by path
    pub async fn list_folder_by_path(&self, path: &str) -> Result<Vec<DriveItem>> {
        let encoded_path = urlencoding::encode(path);
        let url = format!("{}/me/drive/root:{}:/children", GRAPH_API_BASE, encoded_path);
        self.list_folder_by_url(&url).await
    }

    /// List items in a folder 
    async fn list_folder_by_url(&self, url: &str) -> Result<Vec<DriveItem>> {
        let auth_header = self.auth_header().await?;
        
        let full_url = if url.starts_with("http") {
            url.to_string()
        } else {
            format!("{}{}", GRAPH_API_BASE, url)
        };

        let response = self.client
            .get(&full_url)
            .header("Authorization", auth_header)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Failed to list folder: {}", error_text));
        }

        let collection: DriveItemCollection = response.json().await?;
        Ok(collection.value)
    }

    /// Download a file by its download URL and return the file data and metadata
    pub async fn download_file(&self, download_url: &str, onedrive_id: &str, file_name: &str) -> Result<DownloadResult> {
        let response = self.client
            .get(download_url)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!("Failed to download file: {}", response.status()));
        }

        // Extract headers before consuming the response
        let etag = response.headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim_matches('"').to_string());
        
        let content_type = response.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        
        let content_length = response.headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());
        
        let last_modified = response.headers()
            .get("last-modified")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let content = response.bytes().await?;
        
        let result = DownloadResult {
            file_data: content.to_vec(),
            file_name: file_name.to_string(),
            onedrive_id: onedrive_id.to_string(),
            etag,
            mime_type: content_type,
            size: content_length,
            last_modified,
        };
        
        info!("Downloaded file data: {} (ID: {})", file_name, onedrive_id);
        Ok(result)
    }

    /// Upload a file to OneDrive and return the upload result
    pub async fn upload_file(&self, file_data: &[u8], file_name: &str, remote_path: &str) -> Result<UploadResult> {
        let auth_header = self.auth_header().await?;
        
        // Determine parent path
        let parent_path = if remote_path == "/" {
            "/".to_string()
        } else {
            remote_path.to_string()
        };
        
        let url = if parent_path == "/" {
            format!("{}/me/drive/root:/{}:/content", GRAPH_API_BASE, file_name)
        } else {
            let encoded_path = urlencoding::encode(&parent_path);
            format!("{}/me/drive/root:{}:/{}:/content", GRAPH_API_BASE, encoded_path, file_name)
        };

        let response = self.client
            .put(&url)
            .header("Authorization", auth_header)
            .header("Content-Type", "application/octet-stream")
            .body(file_data.to_vec())
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Failed to upload file: {}", error_text));
        }

        let item: DriveItem = response.json().await?;
        
        let result = UploadResult {
            onedrive_id: item.id,
            etag: item.etag,
            web_url: None, // OneDrive API doesn't return web_url in this endpoint
            size: item.size,
        };

        info!("Uploaded file: {} -> {}", file_name, remote_path);
        Ok(result)
    }

    /// Get item by path
    pub async fn get_item_by_path(&self, path: &str) -> Result<DriveItem> {
        let auth_header = self.auth_header().await?;
        let encoded_path = urlencoding::encode(path);
        let url = format!("{}/me/drive/root:{}", GRAPH_API_BASE, encoded_path);

        let response = self.client
            .get(&url)
            .header("Authorization", auth_header)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Failed to get item: {}", error_text));
        }

        let item: DriveItem = response.json().await?;
        Ok(item)
    }

    /// Delete an item by path and return the delete result
    pub async fn delete_item(&self, path: &str) -> Result<DeleteResult> {
        let auth_header = self.auth_header().await?;
        let encoded_path = urlencoding::encode(path);
        let url = format!("{}/me/drive/root:{}", GRAPH_API_BASE, encoded_path);

        let response = self.client
            .delete(&url)
            .header("Authorization", auth_header)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Failed to delete item: {}", error_text));
        }

        let result = DeleteResult {
            success: true,
            item_id: "".to_string(), // OneDrive API doesn't return item ID on delete
            item_path: path.to_string(),
        };

        info!("Deleted item: {}", path);
        Ok(result)
    }

    /// Create a folder and return the creation result
    pub async fn create_folder(&self, parent_path: &str, folder_name: &str) -> Result<CreateFolderResult> {
        let auth_header = self.auth_header().await?;
        
        let url = if parent_path == "/" {
            format!("{}/me/drive/root/children", GRAPH_API_BASE)
        } else {
            let encoded_path = urlencoding::encode(parent_path);
            format!("{}/me/drive/root:{}:/children", GRAPH_API_BASE, encoded_path)
        };

        let body = serde_json::json!({
            "name": folder_name,
            "folder": {},
            "@microsoft.graph.conflictBehavior": "rename"
        });

        let response = self.client
            .post(&url)
            .header("Authorization", auth_header)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Failed to create folder: {}", error_text));
        }

        let item: DriveItem = response.json().await?;
        
        let result = CreateFolderResult {
            onedrive_id: item.id,
            folder_name: folder_name.to_string(),
            web_url: None, // OneDrive API doesn't return web_url in this endpoint
        };

        info!("Created folder: {}", folder_name);
        Ok(result)
    }

    /// Get delta changes for a folder using delta token
    pub async fn get_delta_changes(&self, folder: &str, delta_token: Option<&str>) -> Result<DriveItemCollection> {
        let auth_header = self.auth_header().await?;
        
        let url = if let Some(token) = delta_token {
            // Use existing delta token
            format!("{}/me/drive/root:{}:/delta?token={}", GRAPH_API_BASE, folder, token)
        } else {
            // Initial delta query
            format!("{}/me/drive/root:{}:/delta", GRAPH_API_BASE, folder)
        };

        let response = self.client
            .get(&url)
            .header("Authorization", auth_header)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Failed to get delta changes: {}", error_text));
        }

        let collection: DriveItemCollection = response.json().await?;
        Ok(collection)
    }

    /// Get initial delta state for a folder
    pub async fn get_initial_delta(&self, folder: &str) -> Result<DriveItemCollection> {
        self.get_delta_changes(folder, None).await
    }

    /// Get subsequent delta changes using a token
    pub async fn get_delta_with_token(&self, folder: &str, delta_token: &str) -> Result<DriveItemCollection> {
        self.get_delta_changes(folder, Some(delta_token)).await
    }

    /// Extract delta token from delta link URL
    pub fn extract_delta_token(delta_link: &str) -> Option<String> {
        if let Some(token_start) = delta_link.find("token=") {
            let token = &delta_link[token_start + 6..];
            Some(token.to_string())
        } else {
            None
        }
    }
}