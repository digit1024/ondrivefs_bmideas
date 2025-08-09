use serde::{Deserialize, Serialize};
use zbus::zvariant::Type;

#[derive(Debug, Clone, Deserialize, Serialize, Type, PartialEq, Eq)]
pub enum SyncStatus {
    Running,
    Paused,
    Error,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type, PartialEq, Eq)]
pub struct DaemonStatus {
    pub is_authenticated: bool,
    pub is_connected: bool,
    pub sync_status: SyncStatus,
    pub has_conflicts: bool,
    pub is_mounted: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct SyncQueueItem {
    pub onedrive_id: String,
    pub ino: u64,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct UserProfile {
    pub display_name: String,
    pub given_name: String,
    pub mail: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub enum UserChoice {
    KeepLocal,
    UseRemote,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct ConflictItem {
    pub db_id: i64,
    pub onedrive_id: String,
    pub name: String,
    pub path: String,
    pub error_message: String,
    pub change_type: String, // "Local" or "Remote"
}
#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct ConflictedDriveItem {
    pub name: String,
    pub path: String,
    pub conflicts: Vec<ConflictItem>
} 

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct MediaItem {
    pub onedrive_id: String,
    pub ino: u64,
    pub name: String,
    pub virtual_path: String,
    pub mime_type: Option<String>,
    pub created_date: Option<String>,
    pub last_modified: Option<String>,
} 
