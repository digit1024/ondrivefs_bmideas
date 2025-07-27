use crate::onedrive_service::onedrive_models::DownloadResult;
use anyhow::{Context, Result};
use libc::LOCK_NB;
use log::{info, warn, error, debug};
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
    /// Get the uploads directory
    fn get_local_dir(&self) -> PathBuf;
    
    
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
    pub fn file_exists_in_download(&self, onderive_id: &str) -> bool {
        let download_path = self.config.download_dir().join(onderive_id);
        download_path.exists() && download_path.is_file()
    }
    pub fn file_exists_in_upload(&self, onedrive_id: &str) -> bool {
        let upload_path = self.config.upload_dir().join(onedrive_id);
        upload_path.exists() && upload_path.is_file()
    }
    pub fn get_local_path_if_file_exists(&self, onedrive_id: &str) -> Option<PathBuf> {
        let local_path = self.config.local_dir().join(onedrive_id);
        if local_path.exists() && local_path.is_file() {
            return Some(local_path);
        }
        None
    }
    pub async fn move_downloaded_file_to_local_folder(&self, onedrive_id: &str) -> Result<()> {
        let download = self.get_download_dir().join(onedrive_id);
        let local = self.get_local_dir().join(onedrive_id);
        fs::rename(download, local).await?;
        Ok(())
    }
    pub async fn create_a_snapshot_for_upload(&self, onedrive_id: &str) -> Result<()> {
        
        let local = self.get_local_dir().join(onedrive_id);
        let upload = self.get_upload_dir().join(onedrive_id);
        let local_snapshot = local.clone().with_extension("upload");
        fs::copy(local, local_snapshot.clone()).await?;
        // rename the local file to the upload file
        fs::rename(local_snapshot, upload).await?;
        Ok(())
    }

    /// Create an empty file in the local directory for a given OneDrive ID
    pub async fn create_empty_file(&self, onedrive_id: &str) -> Result<()> {
        let local_path = self.get_local_dir().join(onedrive_id);
        
        // Ensure parent directory exists
        if let Some(parent) = local_path.parent() {
            Self::ensure_directory_exists(parent).await?;
        }
        
        // Create empty file
        fs::write(&local_path, &[]).await
            .with_context(|| format!("Failed to create empty file: {}", local_path.display()))?;
        
        debug!("📄 Created empty file: {} ({})", local_path.display(), onedrive_id);
        Ok(())
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
    fn get_local_dir(&self) -> PathBuf {    
        self.config.local_dir()
    }
}


