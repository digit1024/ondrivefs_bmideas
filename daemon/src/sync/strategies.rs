use super::conflict_resolution::{ConflictResolver, ConflictResolution};
use crate::persistency::processing_item_repository::ProcessingItem;
use onedrive_sync_lib::config::ConflictResolutionStrategy;

/// Always apply remote changes, overwrite local if needed
pub struct AlwaysRemoteStrategy;

impl ConflictResolver for AlwaysRemoteStrategy {
    fn resolve_conflict(&self, _item: &ProcessingItem) -> ConflictResolution {
        ConflictResolution::UseRemote
    }
}

/// Always apply local changes, ignore remote conflicts
pub struct AlwaysLocalStrategy;

impl ConflictResolver for AlwaysLocalStrategy {
    fn resolve_conflict(&self, _item: &ProcessingItem) -> ConflictResolution {
        ConflictResolution::UseLocal
    }
}

/// Manual resolution - wait for user decision
pub struct ManualStrategy;

impl ConflictResolver for ManualStrategy {
    fn resolve_conflict(&self, _item: &ProcessingItem) -> ConflictResolution {
        ConflictResolution::Manual
    }
}

/// Strategy factory
pub struct ConflictResolutionFactory;

impl ConflictResolutionFactory {
    pub fn create_strategy(strategy: &ConflictResolutionStrategy) -> Box<dyn ConflictResolver> {
        match strategy {
            ConflictResolutionStrategy::AlwaysRemote => Box::new(AlwaysRemoteStrategy),
            ConflictResolutionStrategy::AlwaysLocal => Box::new(AlwaysLocalStrategy),
            ConflictResolutionStrategy::Manual => Box::new(ManualStrategy),
        }
    }
} 