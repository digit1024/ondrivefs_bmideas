use crate::persistency::processing_item_repository::{ProcessingItem, UserDecision};
use onedrive_sync_lib::config::ConflictResolutionStrategy;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Trait for conflict resolution strategies
pub trait ConflictResolver {
    fn resolve_conflict(&self, item: &ProcessingItem) -> ConflictResolution;
}

/// Resolution decision for a conflicted item
#[derive(Debug, Clone, PartialEq)]
pub enum ConflictResolution {
    UseRemote,     // Use remote version
    UseLocal,      // Use local version
    Merge,         // Attempt to merge (for text files)
    Skip,          // Skip this item
    Manual,        // Wait for user decision
}

impl ConflictResolution {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConflictResolution::UseRemote => "use_remote",
            ConflictResolution::UseLocal => "use_local",
            ConflictResolution::Merge => "merge",
            ConflictResolution::Skip => "skip",
            ConflictResolution::Manual => "manual",
        }
    }
} 