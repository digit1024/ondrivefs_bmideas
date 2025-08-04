//! Defines the conflict types that can occur during synchronization.

use thiserror::Error;

/// Represents a conflict detected when a change from the remote server
/// conflicts with the state of the local filesystem.
#[derive(Error, Debug, PartialEq, Clone)]
pub enum RemoteConflict {
    #[error("Remote item created, but an item with the same name already exists locally: {0}")]
    CreateOnCreate(String),

    #[error("Remote item was modified, but the local item was also modified (local: {0}, remote: {1})")]
    ModifyOnModify(String, String),

    #[error("Remote item was modified, but the local item was deleted")]
    ModifyOnDelete,

    #[error("Remote item was modified, but its local parent folder was deleted")]
    ModifyOnParentDelete,

    #[error("Remote item was deleted, but the local item has been modified")]
    DeleteOnModify,

    #[error("Remote item was renamed or moved, but an item with the new name already exists locally")]
    RenameOrMoveOnExisting,

    #[error("Remote item was moved, but the local item was also moved to a different location")]
    MoveOnMove,

    #[error("Remote item was moved, but the destination parent folder has been deleted locally")]
    MoveToDeletedParent,
}

/// Represents a conflict detected when a local change (from FUSE)
/// conflicts with the state of the remote server.
#[derive(Error, Debug, PartialEq, Clone)]
pub enum LocalConflict {
    #[error("Local item created, but an item with the same name already exists on the server")]
    CreateOnExisting,

    #[error("Local item was modified, but the corresponding remote item has been deleted")]
    ModifyOnDeleted,

    #[error("Local item was modified, but the remote item was also modified")]
    ModifyOnModified,

    #[error("Local item was deleted, but the remote item has been modified")]
    DeleteOnModified,

    #[error("Local item was renamed or moved, but an item with the target name already exists on the server")]
    RenameOrMoveToExisting,

    #[error("Local item was renamed or moved, but the original source item has been deleted from the server")]
    RenameOrMoveOfDeleted,
}
