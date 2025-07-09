use crate::auth::onedrive_auth::OneDriveAuth;
use crate::onedrive_service::http_client::HttpClient;
use crate::onedrive_service::onedrive_models::{
    CreateFolderResult, DeleteResult, DeltaResponseApi, DownloadResult, DriveItem,
    DriveItemCollection, UploadResult,
};
use anyhow::{Context, Result, anyhow};
use log::info;
use serde_json;
use std::sync::Arc;
use urlencoding;

static THUMBNAIL_SIZE: u64 = 4096;

/// OneDrive API client that handles API operations
/// File system operations are handled by the FileManager trait
pub struct OneDriveClient {
    http_client: HttpClient,
    auth: Arc<OneDriveAuth>,
}

impl OneDriveClient {
    pub fn new(auth: Arc<OneDriveAuth>) -> Result<Self> {
        Ok(Self {
            http_client: HttpClient::new(),
            auth,
        })
    }

    /// Get authorization header with valid token
    async fn auth_header(&self) -> Result<String> {
        let token = self.auth.get_valid_token().await?;

        Ok(format!("Bearer {}", token))
    }

    // /// Download a file by its download URL and return the file data and metadata
    // pub async fn download_file(
    //     &self,
    //     download_url: &str,
    //     onedrive_id: &str,
    //     file_name: &str,
    // ) -> Result<DownloadResult> {
    //     let response = self.http_client.download_file(download_url).await?;

    //     if !response.status().is_success() {
    //         return Err(anyhow!("Failed to download file: {}", response.status()));
    //     }

    //     // Extract headers before consuming the response
    //     let etag = response
    //         .headers()
    //         .get("etag")
    //         .and_then(|v| v.to_str().ok())
    //         .map(|s| s.trim_matches('"').to_string());

    //     let content_type = response
    //         .headers()
    //         .get("content-type")
    //         .and_then(|v| v.to_str().ok())
    //         .map(|s| s.to_string());

    //     let content_length = response
    //         .headers()
    //         .get("content-length")
    //         .and_then(|v| v.to_str().ok())
    //         .and_then(|s| s.parse::<u64>().ok());

    //     let last_modified = response
    //         .headers()
    //         .get("last-modified")
    //         .and_then(|v| v.to_str().ok())
    //         .map(|s| s.to_string());

    //     let content = response.bytes().await?;

    //     let result = DownloadResult {
    //         file_data: content.to_vec(),
    //         file_name: file_name.to_string(),
    //         onedrive_id: onedrive_id.to_string(),
    //         etag,
    //         mime_type: content_type,
    //         size: content_length,
    //         last_modified,
    //     };

    //     info!("Downloaded file data: {} (ID: {})", file_name, onedrive_id);
    //     Ok(result)
    // }

    /// Upload a file to OneDrive and return the upload result
    #[allow(dead_code)]
    pub async fn upload_file(
        &self,
        file_data: &[u8],
        file_name: &str,
        remote_path: &str,
    ) -> Result<UploadResult> {
        let auth_header = self.auth_header().await?;
        let upload_url = self.build_upload_url(file_name, remote_path)?;

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
            onedrive_id: item.id,
            etag: item.etag,
            web_url: None, // OneDrive API doesn't return web_url in this endpoint
            size: item.size,
        };

        info!("Uploaded file: {} -> {}", file_name, remote_path);
        Ok(result)
    }

    /// Build upload URL for file upload
    #[allow(dead_code)]
    fn build_upload_url(&self, file_name: &str, remote_path: &str) -> Result<String> {
        let parent_path = if remote_path == "/" {
            "/".to_string()
        } else {
            remote_path.to_string()
        };

        let url = if parent_path == "/" {
            format!("/me/drive/root:/{}:/content", file_name)
        } else {
            let encoded_path = urlencoding::encode(&parent_path);
            format!("/me/drive/root:{}:/{}:/content", encoded_path, file_name)
        };

        Ok(url)
    }

    /// Get item by path
    #[allow(dead_code)]
    pub async fn get_item_by_path(&self, path: &str) -> Result<DriveItem> {
        let auth_header = self.auth_header().await?;
        let encoded_path = urlencoding::encode(path);
        let url = format!("/me/drive/root:{}", encoded_path);

        let item: DriveItem = self
            .http_client
            .get(&url, &auth_header)
            .await
            .context("Failed to get item by path")?;

        Ok(item)
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
    #[allow(dead_code)]
    pub async fn delete_item(&self, path: &str) -> Result<DeleteResult> {
        let auth_header = self.auth_header().await?;
        let encoded_path = urlencoding::encode(path);
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
    #[allow(dead_code)]
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

    /// Build create folder URL
    #[allow(dead_code)]
    fn build_create_folder_url(&self, parent_path: &str) -> Result<String> {
        let url = if parent_path == "/" {
            "/me/drive/root/children".to_string()
        } else {
            let encoded_path = urlencoding::encode(parent_path);
            format!("/me/drive/root:{}:/children", encoded_path)
        };

        Ok(url)
    }

    /// Build create folder request body
    #[allow(dead_code)]
    fn build_create_folder_body(&self, folder_name: &str) -> serde_json::Value {
        serde_json::json!({
            "name": folder_name,
            "folder": {},
            "@microsoft.graph.conflictBehavior": "rename"
        })
    }

    /// Get delta changes by URL
    pub async fn get_delta_by_url(&self, next_link: &str) -> Result<DeltaResponseApi> {
        let auth_header = self
            .auth_header()
            .await
            .context("Failed to get auth header")?;
        let delta_response = self
            .http_client
            .get(next_link, &auth_header)
            .await
            .context("Failed to get delta by url")?;

        Ok(delta_response)
    }

    /// Get initial delta for root
    #[allow(dead_code)]
    pub async fn get_delta_for_root(&self) -> Result<DeltaResponseApi> {
        let url = "/me/drive/root/delta?select=id,name,eTag,lastModifiedDateTime,size,folder,file,@microsoft.graph.downloadUrl,deleted,parentReference";
        let auth_header = self
            .auth_header()
            .await
            .context("Failed to get auth header")?;
        let delta_response: DeltaResponseApi = self
            .http_client
            .get(url, &auth_header)
            .await
            .context("Failed to get delta for root")?;

        Ok(delta_response)
    }

    /// Get delta changes for a folder using delta token
    #[allow(dead_code)]
    pub async fn get_delta_changes(
        &self,
        folder: &str,
        delta_token: Option<&str>,
    ) -> Result<DriveItemCollection> {
        let url = self.build_delta_url(folder, delta_token);

        let collection: DriveItemCollection = self
            .http_client
            .get(&url, "")
            .await
            .context("Failed to get delta changes")?;

        Ok(collection)
    }

    /// Build delta URL with optional token
    #[allow(dead_code)]
    fn build_delta_url(&self, folder: &str, delta_token: Option<&str>) -> String {
        if let Some(token) = delta_token {
            // Use existing delta token
            format!("/me/drive/root:{}:/delta?token={}", folder, token)
        } else {
            // Initial delta query
            format!("/me/drive/root:{}:/delta", folder)
        }
    }

    /// Get initial delta state for a folder
    #[allow(dead_code)]
    pub async fn get_initial_delta(&self, folder: &str) -> Result<DriveItemCollection> {
        self.get_delta_changes(folder, None).await
    }

    /// Get subsequent delta changes using a token
    #[allow(dead_code)]
    pub async fn get_delta_with_token(
        &self,
        folder: &str,
        delta_token: &str,
    ) -> Result<DriveItemCollection> {
        self.get_delta_changes(folder, Some(delta_token)).await
    }

    /// Extract delta token from delta link URL
    #[allow(dead_code)]
    pub fn extract_delta_token(delta_link: &str) -> Option<String> {
        if let Some(token_start) = delta_link.find("token=") {
            let token = &delta_link[token_start + 6..];
            Some(token.to_string())
        } else {
            None
        }
    }

    /// Download file with optional range and thumbnail support
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

    /// Download partial file (first chunk)
    pub async fn download_file_partial(
        &self,
        download_url: &str,
        item_id: &str,
        filename: &str,
    ) -> Result<DownloadResult> {
        self.download_file_with_options(
            download_url,
            item_id,
            filename,
            Some((0, THUMBNAIL_SIZE - 1)),
        )
        .await
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
}

impl Clone for OneDriveClient {
    fn clone(&self) -> Self {
        Self {
            http_client: self.http_client.clone(),
            auth: self.auth.clone(),
        }
    }
}
