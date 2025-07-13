use crate::onedrive_service::onedrive_models::DownloadResult;
use anyhow::{Context, Result};
use log::{info, warn, error};
use std::path::{Path, PathBuf};
use tokio::fs;
use onedrive_sync_lib::config::ProjectConfig;
use std::sync::Arc;

/// Trait for handling file system operations
pub trait FileManager {
    /// Save a downloaded file to the local file system
    #[allow(dead_code)]
    async fn save_downloaded_file_r(
        &self,
        download_result: &DownloadResult,
        target_path: &Path,
    ) -> Result<()>;

    /// Create a directory
    #[allow(dead_code)]
    async fn create_directory_r(&self, path: &Path) -> Result<()>;

    /// Delete a file
    #[allow(dead_code)]
    async fn delete_file(&self, path: &Path) -> Result<()>;

    /// Delete a directory and its contents
    #[allow(dead_code)]
    async fn delete_directory(&self, path: &Path) -> Result<()>;

    /// Check if a file exists
    #[allow(dead_code)]
    fn file_exists(&self, path: &Path) -> bool;

    /// Check if a directory exists
    #[allow(dead_code)]
    fn directory_exists(&self, path: &Path) -> bool;

    /// Get the downloads directory
    fn get_download_dir(&self) -> PathBuf;
    /// Get the uploads directory
    fn get_upload_dir(&self) -> PathBuf;
    
    
}

/// Trait for synchronous file operations (dyn compatible)
pub trait SyncFileManager {
    /// Check if a file exists
    fn file_exists(&self, path: &Path) -> bool;
    
    /// Convert virtual path to downloaded file path
    fn file_exists_in_download(&self, OneDriveId: &str) -> bool;
    fn file_exists_in_upload(&self, OneDriveId: &str) -> bool;
    fn file_exists_in_locally(&self, OneDriveId: &str) -> bool;// check both

}

/// Default implementation of FileManager
#[derive(Clone)]
pub struct DefaultFileManager {
    config: Arc<ProjectConfig>,
}

impl DefaultFileManager {
    /// Create a new file manager using the provided config
    pub async fn new(config: Arc<ProjectConfig>) -> Result<Self> {
        // Ensure required directories exist
        Self::ensure_directory_exists(&config.download_dir()).await?;
        Self::ensure_directory_exists(&config.upload_dir()).await?;
        Ok(Self { config })
    }

    /// Get the user's home directory
    fn get_home_directory() -> Result<PathBuf> {
        std::env::var("HOME")
            .map(PathBuf::from)
            .map_err(|_| anyhow::anyhow!("HOME environment variable not set"))
    }

    /// Ensure a directory exists, creating it if necessary
    async fn ensure_directory_exists(path: &Path) -> Result<()> {
        if !path.exists() {
            fs::create_dir_all(path)
                .await
                .with_context(|| format!("Failed to create directory: {}", path.display()))?;
            info!("Created directory: {}", path.display());
        }
        Ok(())
    }

    /// Safely strip prefix from path
    fn strip_path_prefix<'a>(path: &'a Path, prefix: &Path) -> &'a Path {
        path.strip_prefix(prefix).unwrap_or(path)
    }
    pub fn file_exists_in_download(&self, OneDriveId: &str) -> bool {
        let download_path = self.config.download_dir().join(OneDriveId);
        download_path.exists() && download_path.is_file()
    }
    pub fn file_exists_in_upload(&self, OneDriveId: &str) -> bool {
        let upload_path = self.config.upload_dir().join(OneDriveId);
        upload_path.exists() && upload_path.is_file()
    }
    pub fn file_exists_in_locally(&self, OneDriveId: &str) -> bool {
        let download_path = self.config.download_dir().join(OneDriveId);
        let upload_path = self.config.upload_dir().join(OneDriveId);
        download_path.exists() && download_path.is_file() || upload_path.exists() && upload_path.is_file()
    }
}

impl FileManager for DefaultFileManager {
    async fn save_downloaded_file_r(
        &self,
        download_result: &DownloadResult,
        target_path: &Path,
    ) -> Result<()> {
        // Create parent directory if it doesn't exist
        if let Some(parent) = target_path.parent() {
            Self::ensure_directory_exists(parent).await?;
        }

        // Write file data
        fs::write(target_path, &download_result.file_data)
            .await
            .with_context(|| format!("Failed to write file: {}", target_path.display()))?;

        info!(
            "✅ Saved downloaded file: {} (ID: {})",
            target_path.display(),
            download_result.onedrive_id
        );
        Ok(())
    }

    async fn create_directory_r(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path)
            .await
            .with_context(|| format!("Failed to create directory: {}", path.display()))?;
        info!("✅ Created directory: {}", path.display());
        Ok(())
    }

    async fn delete_file(&self, path: &Path) -> Result<()> {
        if path.exists() {
            fs::remove_file(path)
                .await
                .with_context(|| format!("Failed to delete file: {}", path.display()))?;
            info!("🗑️ Deleted file: {}", path.display());
        } else {
            warn!("⚠️ Attempted to delete non-existent file: {}", path.display());
        }
        Ok(())
    }

    async fn delete_directory(&self, path: &Path) -> Result<()> {
        if path.exists() {
            fs::remove_dir_all(path)
                .await
                .with_context(|| format!("Failed to delete directory: {}", path.display()))?;
            info!("🗑️ Deleted directory: {}", path.display());
        } else {
            warn!("⚠️ Attempted to delete non-existent directory: {}", path.display());
        }
        Ok(())
    }

    fn file_exists(&self, path: &Path) -> bool {
        path.exists() && path.is_file()
    }

    fn directory_exists(&self, path: &Path) -> bool {
        path.exists() && path.is_dir()
    }

    fn get_download_dir(&self) -> PathBuf {
        self.config.download_dir()
    }
    fn get_upload_dir(&self) -> PathBuf {
        self.config.upload_dir()
    }

    
}

impl SyncFileManager for DefaultFileManager {
    fn file_exists(&self, path: &Path) -> bool {
        path.exists() && path.is_file()
    }
    fn file_exists_in_download(&self, OneDriveId: &str) -> bool {
        let download_path = self.config.download_dir().join(OneDriveId);
        download_path.exists() && download_path.is_file()
    }
    fn file_exists_in_upload(&self, OneDriveId: &str) -> bool {
        let upload_path = self.config.upload_dir().join(OneDriveId);
        upload_path.exists() && upload_path.is_file()
    }
    fn file_exists_in_locally(&self, OneDriveId: &str) -> bool {
        let download_path = self.config.download_dir().join(OneDriveId);
        let upload_path = self.config.upload_dir().join(OneDriveId);
        download_path.exists() && download_path.is_file() || upload_path.exists() && upload_path.is_file()
    }
    
   
}
