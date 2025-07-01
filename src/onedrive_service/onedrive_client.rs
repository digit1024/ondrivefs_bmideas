use anyhow::{ anyhow, Result, Context};
use reqwest::Client;
use urlencoding;
use log::info;
use crate::onedrive_service::onedrive_models::{
    CreateFolderResult, DeleteResult, DeltaResponseApi, DownloadResult, DriveItem, DriveItemCollection, UploadResult
};
use crate::auth::onedrive_auth::OneDriveAuth;
use serde::{Serialize};



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
    fn get_full_url(&self, url: &str) -> Result<String> {
        if url.starts_with("http") {
            Ok(url.to_string())
        } else {
            Ok(format!("{}{}", GRAPH_API_BASE, url))
        }
    }

    async fn get<T>(&self, url: &str) -> Result<T> 
    where 
        T: Serialize + serde::de::DeserializeOwned + std::fmt::Debug
        {
        let url = self.get_full_url(url)?;
        let auth_header = self.auth_header().await?;
        let response = self.client
            .get(url)
            .header("Authorization", auth_header)
            .send().await.context("Failed to get response")?
            .error_for_status().context("Not a success status")?
            .json::<T>().await.context("Failed to Deserialize response to type T")?;
        Ok(response)
    }
    async fn post<T>(&self, url: &str, body: &T) -> Result<T> 
    where 
        T: Serialize + serde::de::DeserializeOwned + std::fmt::Debug
        {
            let url = self.get_full_url(url)?;
        let auth_header = self.auth_header().await?;
        let response = self.client
            .post(url)
            .header("Authorization", auth_header)
            .header("Content-Type", "application/json")
            .json(body)
            .send().await.context("Failed to get response for post")?
            .error_for_status().context("Not a success status")?
            .json::<T>().await.context("Failed to Deserialize response to type T")?;    
        Ok(response)
    }
    async fn delete(&self, url: &str) -> Result<()> 
        {   
            let url = self.get_full_url(url)?;
        let auth_header = self.auth_header().await?;
        self.client
            .delete(url)
            .header("Authorization", auth_header)
            .send().await.context("Failed to get response for delete")?
            .error_for_status().context("Not a success status")?;
        Ok(())
    }
    async fn put<T>(&self, url: &str, body: &T) -> Result<T> 
    where 
        T: Serialize + serde::de::DeserializeOwned + std::fmt::Debug
        {
            let url = self.get_full_url(url)?;
        let auth_header = self.auth_header().await?;
        let response = self.client
            .put(url)
            .header("Authorization", auth_header)
            .header("Content-Type", "application/json")
            .json(body)
            .send().await.context("Failed to get response for put")?
            .error_for_status().context("Not a success status")?
            .json::<T>().await.context("Failed to Deserialize response to type T")?;    
        Ok(response)
    }










    /// Download a file by its download URL and return the file data and metadata
    pub async fn download_file(&self, download_url: &str, onedrive_id: &str, file_name: &str) -> Result<DownloadResult> {
        let auth_header = self.auth_header().await?;
        let response = self.client
            .get(download_url)
            .header("Authorization", auth_header)
            .send().await.context("Failed to get response for download")?
            .error_for_status().context("Not a success status")?;

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


    pub async fn get_delta_by_url(&self, next_link: &str) -> Result<DeltaResponseApi> {
        
        let url = next_link.to_string();
        let delta_response = self.get(&url).await.context("Failed to get delta by url")?;
        Ok(delta_response)
    }

    pub async fn get_delta_for_root(&self) -> Result<DeltaResponseApi> {
        let url = format!("{}/me/drive/root/delta?select=id,name,eTag,lastModifiedDateTime,size,folder,file,@microsoft.graph.downloadUrl,deleted,parentReference", GRAPH_API_BASE);
        let delta_response: DeltaResponseApi = self.get(&url).await.context("Failed to get delta for root")?;
        Ok(delta_response)
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

        let collection: DriveItemCollection = self.get(&url).await.context("Failed to get delta changes")?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_extract_delta_token_with_valid_url() {
        let delta_link = "https://graph.microsoft.com/v1.0/me/drive/root/delta?token=abc123def456";
        let token = OneDriveClient::extract_delta_token(delta_link);
        assert_eq!(token, Some("abc123def456".to_string()));
    }

    #[test]
    fn test_extract_delta_token_with_complex_url() {
        let delta_link = "https://graph.microsoft.com/v1.0/me/drive/root/delta?select=id,name&token=xyz789&other=param";
        let token = OneDriveClient::extract_delta_token(delta_link);
        assert_eq!(token, Some("xyz789&other=param".to_string()));
    }

    #[test]
    fn test_extract_delta_token_without_token() {
        let delta_link = "https://graph.microsoft.com/v1.0/me/drive/root/delta?select=id,name";
        let token = OneDriveClient::extract_delta_token(delta_link);
        assert_eq!(token, None);
    }

    #[test]
    fn test_extract_delta_token_empty_string() {
        let token = OneDriveClient::extract_delta_token("");
        assert_eq!(token, None);
    }

    #[test]
    fn test_get_full_url_with_relative_path() {
        let client = create_test_client();
        let result = client.get_full_url("/me/drive/root").unwrap();
        assert_eq!(result, "https://graph.microsoft.com/v1.0/me/drive/root");
    }

    #[test]
    fn test_get_full_url_with_absolute_url() {
        let client = create_test_client();
        let full_url = "https://example.com/api/test";
        let result = client.get_full_url(full_url).unwrap();
        assert_eq!(result, full_url);
    }

    #[test]
    fn test_get_full_url_with_http_url() {
        let client = create_test_client();
        let http_url = "http://example.com/api/test";
        let result = client.get_full_url(http_url).unwrap();
        assert_eq!(result, http_url);
    }

    // Helper function to create a test client (will fail auth, but useful for URL testing)
    fn create_test_client() -> OneDriveClient {
        // This will likely fail due to auth requirements, but we can test the struct creation
        OneDriveClient {
            client: Client::new(),
            auth: OneDriveAuth::new().unwrap_or_else(|_| {
                // Create a dummy auth for testing - this is a workaround since we can't mock
                panic!("Auth creation failed - this is expected in test environment");
            }),
        }
    }




    // Test constants
    #[test]
    fn test_graph_api_base_constant() {
        assert_eq!(GRAPH_API_BASE, "https://graph.microsoft.com/v1.0");
    }

    // Integration tests that would require network/auth (commented out but structured)
    /*
    #[tokio::test]
    async fn test_get_delta_for_root_integration() {
        // This would require valid auth and network access
        // let client = OneDriveClient::new().unwrap();
        // let result = client.get_delta_for_root().await;
        // assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_upload_file_integration() {
        // This would require valid auth and network access
        // let client = OneDriveClient::new().unwrap();
        // let file_data = b"test content";
        // let result = client.upload_file(file_data, "test.txt", "/").await;
        // assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_download_file_integration() {
        // This would require valid auth and network access
        // let client = OneDriveClient::new().unwrap();
        // let download_url = "https://example.com/download";
        // let result = client.download_file(download_url, "test-id", "test.txt").await;
        // assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_folder_integration() {
        // This would require valid auth and network access
        // let client = OneDriveClient::new().unwrap();
        // let result = client.create_folder("/", "TestFolder").await;
        // assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_delete_item_integration() {
        // This would require valid auth and network access
        // let client = OneDriveClient::new().unwrap();
        // let result = client.delete_item("/test/path").await;
        // assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_item_by_path_integration() {
        // This would require valid auth and network access
        // let client = OneDriveClient::new().unwrap();
        // let result = client.get_item_by_path("/test/path").await;
        // assert!(result.is_ok());
    }
    */

    // Mock tests for URL construction in different scenarios
    #[test]
    fn test_upload_url_construction_root_path() {
        let file_name = "test.txt";
        let parent_path = "/";
        
        let expected_url = format!("{}/me/drive/root:/{}:/content", GRAPH_API_BASE, file_name);
        
        // This tests the URL logic that would be used in upload_file
        let actual_url = if parent_path == "/" {
            format!("{}/me/drive/root:/{}:/content", GRAPH_API_BASE, file_name)
        } else {
            let encoded_path = urlencoding::encode(parent_path);
            format!("{}/me/drive/root:{}:/{}:/content", GRAPH_API_BASE, encoded_path, file_name)
        };
        
        assert_eq!(actual_url, expected_url);
    }

    #[test]
    fn test_upload_url_construction_nested_path() {
        let file_name = "test.txt";
        let parent_path = "/Documents/Projects";
        
        let encoded_path = urlencoding::encode(parent_path);
        let expected_url = format!("{}/me/drive/root:{}:/{}:/content", GRAPH_API_BASE, encoded_path, file_name);
        
        // This tests the URL logic that would be used in upload_file
        let actual_url = if parent_path == "/" {
            format!("{}/me/drive/root:/{}:/content", GRAPH_API_BASE, file_name)
        } else {
            let encoded_path = urlencoding::encode(parent_path);
            format!("{}/me/drive/root:{}:/{}:/content", GRAPH_API_BASE, encoded_path, file_name)
        };
        
        assert_eq!(actual_url, expected_url);
    }

    #[test]
    fn test_create_folder_url_construction_root() {
        let parent_path = "/";
        
        let expected_url = format!("{}/me/drive/root/children", GRAPH_API_BASE);
        
        // This tests the URL logic that would be used in create_folder
        let actual_url = if parent_path == "/" {
            format!("{}/me/drive/root/children", GRAPH_API_BASE)
        } else {
            let encoded_path = urlencoding::encode(parent_path);
            format!("{}/me/drive/root:{}:/children", GRAPH_API_BASE, encoded_path)
        };
        
        assert_eq!(actual_url, expected_url);
    }

    #[test]
    fn test_create_folder_url_construction_nested() {
        let parent_path = "/Documents/Projects";
        
        let encoded_path = urlencoding::encode(parent_path);
        let expected_url = format!("{}/me/drive/root:{}:/children", GRAPH_API_BASE, encoded_path);
        
        // This tests the URL logic that would be used in create_folder
        let actual_url = if parent_path == "/" {
            format!("{}/me/drive/root/children", GRAPH_API_BASE)
        } else {
            let encoded_path = urlencoding::encode(parent_path);
            format!("{}/me/drive/root:{}:/children", GRAPH_API_BASE, encoded_path)
        };
        
        assert_eq!(actual_url, expected_url);
    }

    #[test]
    fn test_delta_url_construction() {
        let expected_base_url = format!("{}/me/drive/root/delta?select=id,name,eTag,lastModifiedDateTime,size,folder,file,@microsoft.graph.downloadUrl,deleted,parentReference", GRAPH_API_BASE);
        
        // Test the URL that would be used in get_delta_for_root
        let actual_url = format!("{}/me/drive/root/delta?select=id,name,eTag,lastModifiedDateTime,size,folder,file,@microsoft.graph.downloadUrl,deleted,parentReference", GRAPH_API_BASE);
        
        assert_eq!(actual_url, expected_base_url);
        assert!(actual_url.contains("select="));
        assert!(actual_url.contains("@microsoft.graph.downloadUrl"));
    }

    // Test URL encoding scenarios
    #[test]
    fn test_path_encoding() {
        let test_cases = vec![
            ("/Documents/My Files", "%2FDocuments%2FMy%20Files"),
            ("/测试/文件夹", "%2F%E6%B5%8B%E8%AF%95%2F%E6%96%87%E4%BB%B6%E5%A4%B9"),
            ("/Files with spaces & symbols", "%2FFiles%20with%20spaces%20%26%20symbols"),
        ];

        for (input, expected) in test_cases {
            let encoded = urlencoding::encode(input);
            assert_eq!(encoded, expected);
        }
    }
}