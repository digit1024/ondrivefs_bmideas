use super::conflict_resolution::{ConflictResolver, ConflictResolution};
use crate::persistency::processing_item_repository::{ProcessingItem, ChangeOperation};
use onedrive_sync_lib::config::ConflictResolutionStrategy;
use chrono::{DateTime, Utc};
use log::{debug, info};

/// Always apply remote changes, overwrite local if needed
pub struct AlwaysRemoteStrategy;

impl ConflictResolver for AlwaysRemoteStrategy {
    fn resolve_conflict(&self, item: &ProcessingItem) -> ConflictResolution {
        debug!("AlwaysRemoteStrategy: Resolving to use remote for {}", 
               item.drive_item.name.as_deref().unwrap_or("unnamed"));
        ConflictResolution::UseRemote
    }
}

/// Always apply local changes, ignore remote conflicts
pub struct AlwaysLocalStrategy;

impl ConflictResolver for AlwaysLocalStrategy {
    fn resolve_conflict(&self, item: &ProcessingItem) -> ConflictResolution {
        debug!("AlwaysLocalStrategy: Resolving to use local for {}", 
               item.drive_item.name.as_deref().unwrap_or("unnamed"));
        ConflictResolution::UseLocal
    }
}

/// Manual resolution - wait for user decision
pub struct ManualStrategy;

impl ConflictResolver for ManualStrategy {
    fn resolve_conflict(&self, item: &ProcessingItem) -> ConflictResolution {
        debug!("ManualStrategy: Deferring to user for {}", 
               item.drive_item.name.as_deref().unwrap_or("unnamed"));
        ConflictResolution::Manual
    }
}

/// Smart strategy that considers operation types and file states
pub struct SmartStrategy;

impl SmartStrategy {
    fn parse_datetime(datetime_str: &str) -> Option<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(datetime_str)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    }
}

impl ConflictResolver for SmartStrategy {
    fn resolve_conflict(&self, item: &ProcessingItem) -> ConflictResolution {
        info!("SmartStrategy: Analyzing conflict for {} (operation: {:?})", 
              item.drive_item.name.as_deref().unwrap_or("unnamed"),
              item.change_operation);

        // Handle different operation combinations
        match item.change_operation {
            ChangeOperation::Delete => {
                // If item is deleted on one side, respect the deletion
                debug!("SmartStrategy: Item marked for deletion, using delete operation");
                ConflictResolution::UseLocal // Or UseRemote based on which side deleted
            },
            ChangeOperation::Create => {
                // Both sides created the same file - compare timestamps
                if let Some(last_modified) = item.drive_item.last_modified.as_ref() {
                    if let Some(_) = Self::parse_datetime(last_modified) {
                        debug!("SmartStrategy: Create conflict, using newest version");
                        ConflictResolution::UseNewest
                    } else {
                        ConflictResolution::Manual
                    }
                } else {
                    ConflictResolution::Manual
                }
            },
            ChangeOperation::Update => {
                // Check if file is downloaded
                if item.drive_item.size.unwrap_or(0) > 0 {
                    // File has content, use timestamp comparison
                    debug!("SmartStrategy: Update conflict on downloaded file, using newest");
                    ConflictResolution::UseNewest
                } else {
                    // File not downloaded yet, prefer remote
                    debug!("SmartStrategy: Update conflict on not-downloaded file, using remote");
                    ConflictResolution::UseRemote
                }
            },
            ChangeOperation::Move{ ..} => {
                // For moves, manual resolution is safest
                debug!("SmartStrategy: Move conflict, requiring manual resolution");
                ConflictResolution::Manual
            },
            ChangeOperation::Rename{ ..} => {
                // For renames, keep both with different names
                debug!("SmartStrategy: Rename conflict, keeping both");
                ConflictResolution::KeepBoth
            },
            ChangeOperation::NoChange => {
                debug!("SmartStrategy: No change conflict, using remote");
                ConflictResolution::UseRemote
            }
        }
    }
}

/// Timestamp-based strategy
pub struct TimestampStrategy {
    pub use_newest: bool,
}

impl ConflictResolver for TimestampStrategy {
    fn resolve_conflict(&self, item: &ProcessingItem) -> ConflictResolution {
        debug!("TimestampStrategy: Resolving based on {} timestamp", 
               if self.use_newest { "newest" } else { "oldest" });
        
        if self.use_newest {
            ConflictResolution::UseNewest
        } else {
            ConflictResolution::UseOldest
        }
    }
}

/// Size-based strategy
pub struct SizeStrategy {
    pub use_largest: bool,
}

impl ConflictResolver for SizeStrategy {
    fn resolve_conflict(&self, item: &ProcessingItem) -> ConflictResolution {
        debug!("SizeStrategy: Resolving based on {} size", 
               if self.use_largest { "largest" } else { "smallest" });
        
        if self.use_largest {
            ConflictResolution::UseLargest
        } else {
            ConflictResolution::UseSmallest
        }
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

    pub fn create_smart_strategy() -> Box<dyn ConflictResolver> {
        Box::new(SmartStrategy)
    }

    pub fn create_timestamp_strategy(use_newest: bool) -> Box<dyn ConflictResolver> {
        Box::new(TimestampStrategy { use_newest })
    }

    pub fn create_size_strategy(use_largest: bool) -> Box<dyn ConflictResolver> {
        Box::new(SizeStrategy { use_largest })
    }
} 