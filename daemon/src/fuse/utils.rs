//! Utility functions for FUSE filesystem implementation

use tokio::runtime::Handle;

/// Synchronously await a future in the current async context
/// This is used to bridge async and sync code in FUSE operations
pub fn sync_await<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    tokio::task::block_in_place(|| Handle::current().block_on(future))
}

/// FUSE capability for readdirplus
pub const FUSE_CAP_READDIRPLUS: u32 = 0x00000010;
