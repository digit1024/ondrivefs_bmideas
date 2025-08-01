pub mod conflict_resolution;
pub mod strategies;
pub mod sync_processor;
pub mod sync_strategy;

#[cfg(test)]
mod conflict_resolution_tests;

pub use conflict_resolution::*;
pub use strategies::*;
pub use sync_processor::*;
pub use sync_strategy::*;

// Re-export types for convenience
pub use crate::persistency::processing_item_repository::{ChangeType, ChangeOperation, UserDecision}; 