use crate::onedrive_service::onedrive_models::DownloadResult;
use anyhow::{Context, Result};
use log::{info, warn, error};
use std::path::{Path, PathBuf};
use tokio::fs;

/// Default directory names for OneDrive storage
const ONEDRIVE_DIR: &str = ".onedrive";
const TEMP_DIR: &str = "tmp";
const DOWNLOADS_DIR: &str = "downloads";
const CACHE_DIR: &str = "cache";

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

    /// Get the temporary download directory
    fn get_temp_download_dir(&self) -> PathBuf;

    /// Get the cache directory
    fn get_cache_dir(&self) -> PathBuf;
    
    /// Convert cache path to virtual path
    fn cache_path_to_virtual_path(&self, cache_path: &Path) -> PathBuf;
    
    /// Convert virtual path to cache path
    #[allow(dead_code)]
    fn virtual_path_to_cache_path(&self, virtual_path: &Path) -> PathBuf;
    
    /// Convert virtual path to downloaded file path
    fn virtual_path_to_downloaded_path(&self, virtual_path: &Path) -> PathBuf;
}

/// Default implementation of FileManager
#[derive(Clone)]
pub struct DefaultFileManager {
    temp_dir: PathBuf,  // Directory to store temporary files
    cache_dir: PathBuf, // Directory to store metadata for files
}

impl DefaultFileManager {
    /// Create a new file manager with default directories
    pub async fn new() -> Result<Self> {
        let home_dir = Self::get_home_directory()?;
        let onedrive_base = home_dir.join(ONEDRIVE_DIR);
        
        let temp_dir = onedrive_base.join(TEMP_DIR).join(DOWNLOADS_DIR);
        let cache_dir = onedrive_base.join(CACHE_DIR);

        // Create directories if they don't exist
        Self::ensure_directory_exists(&temp_dir).await?;
        Self::ensure_directory_exists(&cache_dir).await?;

        Ok(Self {
            temp_dir,
            cache_dir,
        })
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

    fn get_temp_download_dir(&self) -> PathBuf {
        self.temp_dir.clone()
    }
    
    fn get_cache_dir(&self) -> PathBuf {
        self.cache_dir.clone()
    }

    fn cache_path_to_virtual_path(&self, cache_path: &Path) -> PathBuf {
        let relative_path = Self::strip_path_prefix(cache_path, &self.cache_dir);
        
        if relative_path == Path::new("") {
            // Root directory case
            PathBuf::from("/")
        } else {
            // Add leading slash to make it a proper virtual path
            PathBuf::from("/").join(relative_path)
        }
    }

    fn virtual_path_to_cache_path(&self, virtual_path: &Path) -> PathBuf {
        // Remove leading slash
        let virtual_path = Self::strip_path_prefix(virtual_path, Path::new("/"));
        self.cache_dir.join(virtual_path)
    }

    fn virtual_path_to_downloaded_path(&self, virtual_path: &Path) -> PathBuf {
        // Remove leading slash
        let virtual_path = Self::strip_path_prefix(virtual_path, Path::new("/"));
        self.temp_dir.join(virtual_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_file_manager_creation() {
        let file_manager = DefaultFileManager::new().await;
        assert!(file_manager.is_ok());
    }

    #[test]
    fn test_path_operations() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        
        let file_manager = DefaultFileManager {
            temp_dir: temp_dir.path().to_path_buf(),
            cache_dir: cache_dir.path().to_path_buf(),
        };

        // Test virtual path to cache path conversion
        let virtual_path = Path::new("/test/file.txt");
        let cache_path = file_manager.virtual_path_to_cache_path(virtual_path);
        assert_eq!(cache_path, cache_dir.path().join("test/file.txt"));

        // Test cache path to virtual path conversion
        let cache_path = cache_dir.path().join("test/file.txt");
        let virtual_path = file_manager.cache_path_to_virtual_path(&cache_path);
        assert_eq!(virtual_path, PathBuf::from("/test/file.txt"));
    }

    #[test]
    fn test_file_exists_checks() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        
        let file_manager = DefaultFileManager {
            temp_dir: temp_dir.path().to_path_buf(),
            cache_dir: cache_dir.path().to_path_buf(),
        };

        // Test directory existence
        assert!(file_manager.directory_exists(temp_dir.path()));
        assert!(!file_manager.directory_exists(&temp_dir.path().join("nonexistent")));

        // Test file existence
        let test_file = temp_dir.path().join("test.txt");
        std::fs::write(&test_file, "test").unwrap();
        assert!(file_manager.file_exists(&test_file));
        assert!(!file_manager.file_exists(&temp_dir.path().join("nonexistent.txt")));
    }
}
