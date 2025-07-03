//! Retry logic for file system operations

use anyhow::Result;
use log::{error, info, warn};
use std::path::Path;
use std::time::Duration;

const MAX_RETRIES: u32 = 5;
const RETRY_DELAY_MS: u64 = 100;

/// Retry configuration for file operations
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub delay_ms: u32,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: MAX_RETRIES,
            delay_ms: RETRY_DELAY_MS as u32,
        }
    }
}

/// Force remove a file with retry logic for busy files
pub async fn force_remove_file<P: AsRef<Path>>(path: P) -> Result<()> {
    force_remove_file_with_config(path, RetryConfig::default()).await
}

/// Force remove a file with custom retry configuration
pub async fn force_remove_file_with_config<P: AsRef<Path>>(
    path: P,
    config: RetryConfig,
) -> Result<()> {
    let path = path.as_ref();
    let mut retry_count = 0;
    
    while retry_count < config.max_retries {
        match std::fs::remove_file(path) {
            Ok(()) => {
                info!("Successfully removed file: {}", path.display());
                return Ok(());
            }
            Err(e) => {
                retry_count += 1;
                match e.kind() {
                    std::io::ErrorKind::NotFound => {
                        info!("File already does not exist: {}", path.display());
                        return Ok(());
                    }
                    std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Other => {
                        if retry_count < config.max_retries {
                            warn!(
                                "File {} is busy, retrying in {}ms (attempt {}/{}): {}",
                                path.display(),
                                retry_count * config.delay_ms,
                                retry_count,
                                config.max_retries,
                                e
                            );
                            tokio::time::sleep(Duration::from_millis((retry_count * config.delay_ms) as u64)).await;
                            continue;
                        }
                    }
                    _ => {}
                }
                
                if retry_count >= config.max_retries {
                    error!("Failed to remove file {} after {} attempts: {}", path.display(), config.max_retries, e);
                    return Err(anyhow::anyhow!("Failed to remove file after {} attempts: {}", config.max_retries, e));
                }
            }
        }
    }
    
    unreachable!()
}

/// Force remove a directory with retry logic for busy directories
pub async fn force_remove_dir_all<P: AsRef<Path>>(path: P) -> Result<()> {
    force_remove_dir_all_with_config(path, RetryConfig::default()).await
}

/// Force remove a directory with custom retry configuration
pub async fn force_remove_dir_all_with_config<P: AsRef<Path>>(
    path: P,
    config: RetryConfig,
) -> Result<()> {
    let path = path.as_ref();
    let mut retry_count = 0;
    
    while retry_count < config.max_retries {
        match std::fs::remove_dir_all(path) {
            Ok(()) => {
                info!("Successfully removed directory: {}", path.display());
                return Ok(());
            }
            Err(e) => {
                retry_count += 1;
                match e.kind() {
                    std::io::ErrorKind::NotFound => {
                        info!("Directory already does not exist: {}", path.display());
                        return Ok(());
                    }
                    std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Other => {
                        if retry_count < config.max_retries {
                            warn!(
                                "Directory {} is busy, retrying in {}ms (attempt {}/{}): {}",
                                path.display(),
                                retry_count * config.delay_ms,
                                retry_count,
                                config.max_retries,
                                e
                            );
                            tokio::time::sleep(Duration::from_millis((retry_count * config.delay_ms) as u64)).await;
                            continue;
                        }
                    }
                    _ => {}
                }
                
                if retry_count >= config.max_retries {
                    error!("Failed to remove directory {} after {} attempts: {}", path.display(), config.max_retries, e);
                    return Err(anyhow::anyhow!("Failed to remove directory after {} attempts: {}", config.max_retries, e));
                }
            }
        }
    }
    
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_force_remove_file_not_exists() {
        let temp_dir = tempdir().unwrap();
        let non_existent_file = temp_dir.path().join("non_existent.txt");
        
        // Should succeed without error for non-existent file
        let result = force_remove_file(&non_existent_file).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_force_remove_dir_not_exists() {
        let temp_dir = tempdir().unwrap();
        let non_existent_dir = temp_dir.path().join("non_existent_dir");
        
        // Should succeed without error for non-existent directory
        let result = force_remove_dir_all(&non_existent_dir).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_force_remove_file_exists() {
        let temp_dir = tempdir().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        std::fs::write(&test_file, "test content").unwrap();
        
        // Should succeed for existing file
        let result = force_remove_file(&test_file).await;
        assert!(result.is_ok());
        assert!(!test_file.exists());
    }

    #[tokio::test]
    async fn test_force_remove_dir_exists() {
        let temp_dir = tempdir().unwrap();
        let test_dir = temp_dir.path().join("test_dir");
        std::fs::create_dir(&test_dir).unwrap();
        let test_file = test_dir.join("test.txt");
        std::fs::write(&test_file, "test content").unwrap();
        
        // Should succeed for existing directory
        let result = force_remove_dir_all(&test_dir).await;
        assert!(result.is_ok());
        assert!(!test_dir.exists());
    }
} 