use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::Deserialize;
use std::path::Path;
use tokio::fs;
use urlencoding;
use log::info;

use crate::auth::onedrive_auth::OneDriveAuth;
use crate::metadata_manager_for_files::MetadataManagerForFiles;


const GRAPH_API_BASE: &str = "https://graph.microsoft.com/v1.0";

#[derive(Debug, Deserialize)]
pub struct ParentReference {
    pub id: String,
    pub path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DriveItem {
    pub id: String,
    pub name: Option<String>,
    #[serde(rename = "eTag")]
    pub etag: Option<String>,
    #[serde(rename = "lastModifiedDateTime")]
    pub last_modified: Option<String>,
    pub size: Option<u64>,
    pub folder: Option<FolderFacet>,
    pub file: Option<FileFacet>,
    #[serde(rename = "@microsoft.graph.downloadUrl")]
    pub download_url: Option<String>,
    /// Indicates if the item was deleted
    pub deleted: Option<serde_json::Value>,
    #[serde(rename = "parentReference")]
    pub parent_reference: Option<ParentReference>,
}


#[derive(Debug, Deserialize)]
pub struct FolderFacet {
    #[serde(rename = "childCount")]
    pub child_count: u32,
}

#[derive(Debug, Deserialize)]
pub struct FileFacet {
    #[serde(rename = "mimeType")]
    pub mime_type: Option<String>,  // Changed from String to Option<String>
}

#[derive(Debug, Deserialize)]
pub struct DriveItemCollection {
    pub value: Vec<DriveItem>,
    #[serde(rename = "@odata.nextLink")]
    pub next_link: Option<String>,
    #[serde(rename = "@odata.deltaLink")]
    pub delta_link: Option<String>,
}

pub struct OneDriveClient {
    client: Client,
    auth: OneDriveAuth,
    metadata_manager: MetadataManagerForFiles,
}

impl OneDriveClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: Client::new(),
            auth: OneDriveAuth::new()?,
            metadata_manager: MetadataManagerForFiles::new()?,
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

    /// Download a file by its download URL and store metadata
    pub async fn download_file(&self, download_url: &str, local_path: &Path, onedrive_id: &str, name: &str, etag: Option<&String>) -> Result<()> {
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
        
        // Store metadata after successful download
        self.metadata_manager.add_metadata_for_file(onedrive_id, local_path)?;
        // Store etag/id in path_to_onedrive_metadata
        if let Some(etag) = etag {
            let meta = crate::metadata_manager_for_files::OnedriveFileMeta {
                etag: etag.to_string(),
                id: onedrive_id.to_string(),
            };
            self.metadata_manager.set_onedrive_file_meta(&local_path.to_string_lossy(), &meta)?;
        }
        
        info!("Downloaded file: {} (ID: {})", local_path.to_string_lossy(), onedrive_id);
        Ok(())
    }

    /// Download a file by its download URL (legacy method without metadata)
    pub async fn download_file_legacy(&self, download_url: &str, local_path: &Path) -> Result<()> {
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
        info!("Downloaded file: {}", local_path.to_string_lossy());
        Ok(())
    }

    /// Upload a file to OneDrive
    pub async fn upload_file(&self, local_path: &Path, remote_path: &str) -> Result<()> {
        let auth_header = self.auth_header().await?;
        
        // Read file content
        let content = fs::read(local_path).await?;
        
        // Get file name from local path
        let file_name = local_path.file_name()
            .ok_or_else(|| anyhow!("Invalid file path"))?
            .to_string_lossy();
        
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
            .body(content)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Failed to upload file: {}", error_text));
        }

        info!("Uploaded file: {} -> {}", local_path.display(), remote_path);
        Ok(())
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

    /// Delete an item by path
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

        info!("Deleted item: {}", path);
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
        info!("Created folder: {}", folder_name);
        Ok(item)
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

    /// Get metadata manager reference
    pub fn metadata_manager(&self) -> &MetadataManagerForFiles {
        &self.metadata_manager
    }

    /// Handle delta changes with metadata tracking
    pub async fn handle_delta_changes(&self, delta_collection: &DriveItemCollection, local_root: &Path) -> Result<()> {
        for item in &delta_collection.value {
            if item.deleted.is_some() {
                // Handle deleted item
                if let Some(local_path_str) = self.metadata_manager.get_local_path_from_one_drive_id(&item.id)? {
                    let local_path = Path::new(&local_path_str);
                    if local_path.exists() {
                        fs::remove_file(local_path).await?;
                        info!("Deleted local file: {}", local_path_str);
                    }
                    // Delete metadata
                    self.metadata_manager.delete_metadata_for_file(&item.id)?;
                }
            } else {
                // Handle created/modified item
                if let Some(name) = &item.name {
                    let local_path = local_root.join(name);
                    
                    if item.file.is_some() {
                        // It's a file - download it
                        if let Some(download_url) = &item.download_url {
                            self.download_file(download_url, &local_path, &item.id, name, None).await?;
                        }
                    } else if item.folder.is_some() {
                        // It's a folder - create it
                        fs::create_dir_all(&local_path).await?;
                        self.metadata_manager.add_metadata_for_file(&item.id, &local_path)?;
                        info!("Created folder: {} (ID: {})", name, item.id);
                    }
                }
            }
        }
        Ok(())
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