use crate::persistency::processing_item_repository::{ProcessingItem, UserDecision};
use anyhow::Result;
use onedrive_sync_lib::config::ConflictResolutionStrategy;
use serde::{Deserialize, Serialize};

/// Trait for conflict resolution strategies
pub trait ConflictResolver {
    fn resolve_conflict(&self, item: &ProcessingItem) -> ConflictResolution;
}

/// Resolution decision for a conflicted item
#[derive(Debug, Clone, PartialEq)]
pub enum ConflictResolution {
    UseRemote, // Use remote version
    UseLocal,  // Use local version
    Skip,      // Skip this item
    Manual,    // Wait for user decision
}

impl ConflictResolution {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            ConflictResolution::UseRemote => "use_remote",
            ConflictResolution::UseLocal => "use_local",

            ConflictResolution::Skip => "skip",
            ConflictResolution::Manual => "manual",
        }
    }
}
