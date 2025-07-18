//! Shared types for OneDrive sync DBus interface

use serde::{Deserialize, Serialize};


/// Sync status enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncStatus {
    Running,
    Paused,
    Error(String),
}

impl std::fmt::Display for SyncStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncStatus::Running => write!(f, "running"),
            SyncStatus::Paused => write!(f, "paused"),
            SyncStatus::Error(e) => write!(f, "error: {}", e),
        }
    }
}

/// Sync progress information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncProgress {
    pub current_files: u32,
    pub total_files: u32,
    pub current_bytes: u64,
    pub total_bytes: u64,
}

/// Sync metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncMetrics {
    pub status: SyncStatus,
    pub progress: SyncProgress,
    pub queue_size: u32,
    pub last_sync_time: Option<String>,
    pub sync_folders: Vec<String>,
}

/// File operation types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileOperation {
    Download,
    Upload,
    Delete,
    Modify,
}

/// File event information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEvent {
    pub filename: String,
    pub operation: FileOperation,
    pub timestamp: String,
}

/// Error types for DBus operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncError {
    AuthenticationFailed(String),
    NetworkError(String),
    FileSystemError(String),
    ConfigurationError(String),
    UnknownError(String),
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::AuthenticationFailed(msg) => write!(f, "Authentication failed: {}", msg),
            SyncError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            SyncError::FileSystemError(msg) => write!(f, "File system error: {}", msg),
            SyncError::ConfigurationError(msg) => write!(f, "Configuration error: {}", msg),
            SyncError::UnknownError(msg) => write!(f, "Unknown error: {}", msg),
        }
    }
}

impl std::error::Error for SyncError {}

/// Result type for sync operations
pub type SyncResult<T> = Result<T, SyncError>;

/// Convert anyhow error to sync error
pub fn anyhow_to_sync_error(err: anyhow::Error) -> SyncError {
    SyncError::UnknownError(err.to_string())
} 