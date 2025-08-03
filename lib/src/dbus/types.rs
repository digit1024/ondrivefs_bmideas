use serde::{Deserialize, Serialize};
use zbus::zvariant::Type;

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub enum SyncStatus {
    Running,
    Paused,
    Error,
}
#[derive(Debug, Clone, Deserialize, Serialize, Type)]
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
