use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::Deserialize;
use std::path::Path;
use tokio::fs;
use urlencoding;

use crate::onedrive_auth::OneDriveAuth;

const GRAPH_API_BASE: &str = "https://graph.microsoft.com/v1.0";

#[derive(Debug, Deserialize)]
pub struct DriveItem {
    pub id: String,
    pub name: String,
    #[serde(rename = "lastModifiedDateTime")]
    pub last_modified: String,
    pub size: Option<u64>,
    pub folder: Option<FolderFacet>,
    pub file: Option<FileFacet>,
    #[serde(rename = "@microsoft.graph.downloadUrl")]
    pub download_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FolderFacet {
    #[serde(rename = "childCount")]
    pub child_count: u32,
}

#[derive(Debug, Deserialize)]
pub struct FileFacet {
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

#[derive(Debug, Deserialize)]
pub struct DriveItemCollection {
    pub value: Vec<DriveItem>,
    #[serde(rename = "@odata.nextLink")]
    pub next_link: Option<String>,
}

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
        self.list_folder("/me/drive/root/children").await
    }

    /// List items in a specific folder by path
    pub async fn list_folder_by_path(&self, path: &str) -> Result<Vec<DriveItem>> {
        let encoded_path = urlencoding::encode(path);
        let url = format!("{}/me/drive/root:{}:/children", GRAPH_API_BASE, encoded_path);
        self.list_folder(&url).await
    }

    /// List items in a folder (generic)
    async fn list_folder(&self, url: &str) -> Result<Vec<DriveItem>> {
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

    /// Download a file by its download URL
    pub async fn download_file(&self, download_url: &str, local_path: &Path) -> Result<()> {
        let response = self.client
            .get(download_url)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!("Failed to download file: {}", response.status()));
        }

        let content = response.bytes().await?;
        
        // Create parent directory if it doesn't exist
        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        fs::write(local_path, content).await?;
        Ok(())
    }

    /// Upload a file to OneDrive
    pub async fn upload_file(&self, local_path: &Path, remote_path: &str) -> Result<DriveItem> {
        let auth_header = self.auth_header().await?;
        let file_content = fs::read(local_path).await?;
        
        let encoded_path = urlencoding::encode(remote_path);
        let url = format!("{}/me/drive/root:{}:/content", GRAPH_API_BASE, encoded_path);

        let response = self.client
            .put(&url)
            .header("Authorization", auth_header)
            .header("Content-Type", "application/octet-stream")
            .body(file_content)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Failed to upload file: {}", error_text));
        }

        let item: DriveItem = response.json().await?;
        Ok(item)
    }

    /// Get file metadata by path
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

    /// Delete a file or folder by path
    pub async fn delete_item(&self, path: &str) -> Result<()> {
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

        Ok(())
    }

    /// Create a folder
    pub async fn create_folder(&self, parent_path: &str, folder_name: &str) -> Result<DriveItem> {
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
        Ok(item)
    }
}