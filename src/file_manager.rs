use anyhow::Result;
use std::path::{Path, PathBuf};
use tokio::fs;
use log::info;
use crate::onedrive_service::onedrive_models::DownloadResult;
use crate::metadata_manager_for_files::{MetadataManagerForFiles, OnedriveFileMeta};









/// Trait for handling file system operations
pub trait FileManager {
    /// Save a downloaded file to the local file system
    async fn save_downloaded_file(&self, download_result: &DownloadResult, target_path: &Path) -> Result<()>;
    
    /// Create a directory
    async fn create_directory(&self, path: &Path) -> Result<()>;
    
    /// Delete a file
    async fn delete_file(&self, path: &Path) -> Result<()>;
    
    /// Delete a directory and its contents
    async fn delete_directory(&self, path: &Path) -> Result<()>;
    
    /// Check if a file exists
    fn file_exists(&self, path: &Path) -> bool;
    
    /// Check if a directory exists
    fn directory_exists(&self, path: &Path) -> bool;
    
    /// Get the temporary download directory
    fn get_temp_download_dir(&self) -> PathBuf;

    /// Get the cache directory
    fn get_cache_dir(&self) -> PathBuf;


    
}

/// Default implementation of FileManager
pub struct DefaultFileManager {
    metadata_manager: MetadataManagerForFiles,
    temp_dir: PathBuf,
    cache_dir: PathBuf,
}

impl DefaultFileManager {
    pub async fn new(metadata_manager: MetadataManagerForFiles) -> Result<Self> {
        let home_dir = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"));
        
        let temp_dir = home_dir.join(".onedrive").join("tmp").join("downloads");
        let cache_dir = home_dir.join(".onedrive").join("cache");

        // Create temp directory if it doesn't exist
        if !temp_dir.exists() {
            fs::create_dir_all(&temp_dir).await?;
        }
        if !cache_dir.exists() {
            fs::create_dir_all(&cache_dir).await?;
        }
        
        Ok(Self {
            metadata_manager,
            temp_dir,
            cache_dir,
        })
    }
    
    /// Get a reference to the metadata manager
    pub fn metadata_manager(&self) -> &MetadataManagerForFiles {
        &self.metadata_manager
    }
}

impl FileManager for DefaultFileManager {
    async fn save_downloaded_file(&self, download_result: &DownloadResult, target_path: &Path) -> Result<()> {
        // Create parent directory if it doesn't exist
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Write file data
        fs::write(target_path, &download_result.file_data).await?;
        
        
        
        
        // Store etag if available
        if let Some(etag) = &download_result.etag {
            let meta = OnedriveFileMeta {
                etag: etag.to_string(),
                id: download_result.onedrive_id.clone(),
            };
        
        }
        
        info!("Saved downloaded file: {} (ID: {})", target_path.display(), download_result.onedrive_id);
        Ok(())
    }
    
    async fn create_directory(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path).await?;
        info!("Created directory: {}", path.display());
        Ok(())
    }
    
    async fn delete_file(&self, path: &Path) -> Result<()> {
        if path.exists() {
            fs::remove_file(path).await?;
            info!("Deleted file: {}", path.display());
        }
        Ok(())
    }
    
    async fn delete_directory(&self, path: &Path) -> Result<()> {
        if path.exists() {
            fs::remove_dir_all(path).await?;
            info!("Deleted directory: {}", path.display());
        }
        Ok(())
    }
    
    fn file_exists(&self, path: &Path) -> bool {
        path.exists() && path.is_file()
    }
    
    fn directory_exists(&self, path: &Path) -> bool {
        path.exists() && path.is_dir()
    }
    
    fn get_temp_download_dir(&self) -> PathBuf {
        self.temp_dir.clone()
    }
    fn get_cache_dir(&self) -> PathBuf {
        self.cache_dir.clone()
    }
} 