use crate::persistency::processing_item_repository::{ProcessingItem, UserDecision};
use onedrive_sync_lib::config::ConflictResolutionStrategy;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Trait for conflict resolution strategies
pub trait ConflictResolver: Send + Sync {
    fn resolve_conflict(&self, item: &ProcessingItem) -> ConflictResolution;
}

/// Resolution decision for a conflicted item
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConflictResolution {
    UseRemote,          // Use remote version
    UseLocal,           // Use local version
    Skip,               // Skip this item
    Manual,             // Wait for user decision
    UseNewest,          // Use the newest version (by timestamp)
    UseOldest,          // Use the oldest version (by timestamp)
    UseLargest,         // Use the largest file
    UseSmallest,        // Use the smallest file
    KeepBoth,           // Keep both files with different names
}

impl ConflictResolution {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConflictResolution::UseRemote => "use_remote",
            ConflictResolution::UseLocal => "use_local",
            ConflictResolution::Skip => "skip",
            ConflictResolution::Manual => "manual",
            ConflictResolution::UseNewest => "use_newest",
            ConflictResolution::UseOldest => "use_oldest",
            ConflictResolution::UseLargest => "use_largest",
            ConflictResolution::UseSmallest => "use_smallest",
            ConflictResolution::KeepBoth => "keep_both",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "use_remote" => Some(ConflictResolution::UseRemote),
            "use_local" => Some(ConflictResolution::UseLocal),
            "skip" => Some(ConflictResolution::Skip),
            "manual" => Some(ConflictResolution::Manual),
            "use_newest" => Some(ConflictResolution::UseNewest),
            "use_oldest" => Some(ConflictResolution::UseOldest),
            "use_largest" => Some(ConflictResolution::UseLargest),
            "use_smallest" => Some(ConflictResolution::UseSmallest),
            "keep_both" => Some(ConflictResolution::KeepBoth),
            _ => None,
        }
    }
}
