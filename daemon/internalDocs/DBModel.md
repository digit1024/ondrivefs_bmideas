# Database Models & Repositories

## Core Data Structures

### DriveItemWithFuse
**File**: `persistency/types.rs`

Main entity combining OneDrive metadata with FUSE filesystem data:

```rust
pub struct DriveItemWithFuse {
    pub drive_item: DriveItem,           // OneDrive API response
    pub fuse_metadata: FuseMetadata,     // FUSE-specific data
}
```

**Key Methods**:
- `compute_virtual_path()`: Generates filesystem path from parent reference
- `update_fuse_metadata()`: Updates FUSE metadata
- `set_virtual_ino()`: Sets virtual inode number

### FuseMetadata
**File**: `persistency/types.rs`

FUSE filesystem metadata:

```rust
pub struct FuseMetadata {
    pub virtual_ino: Option<u64>,        // Virtual inode number
    pub parent_ino: Option<u64>,         // Parent inode number
    pub virtual_path: Option<String>,    // Filesystem path
    pub file_source: Option<FileSource>, // Data source (Remote/Local/Merged)
    pub sync_status: Option<String>,     // Synchronization status
}
```

### FileSource Enum
**File**: `persistency/types.rs`

Indicates data origin:
- **Remote**: From OneDrive API
- **Local**: From local filesystem changes
- **Merged**: Combined from both sources

### DownloadQueueItem
**File**: `persistency/types.rs`

Download tracking:

```rust
pub struct DownloadQueueItem {
    pub id: i64,                         // Database ID
    pub onedrive_id: String,             // OneDrive item ID
    pub local_path: PathBuf,             // Local file path
    pub priority: i32,                   // Download priority
    pub status: String,                  // Current status
    pub retry_count: i32,                // Retry attempts
    pub ino: u64,                        // FUSE inode
    pub name: String,                    // File name
    pub virtual_path: Option<String>,    // Virtual path
}
```

### ProcessingItem
**File**: `persistency/processing_item_repository.rs`

Change tracking for synchronization:

```rust
pub struct ProcessingItem {
    pub id: Option<i64>,                 // Database ID
    pub drive_item: DriveItem,           // OneDrive item
    pub change_type: ChangeType,         // Type of change
    pub status: ProcessingStatus,        // Processing status
    pub validation_errors: Vec<String>,  // Validation errors
    pub created_at: String,              // Creation timestamp
    pub updated_at: String,              // Last update
}
```

**ChangeType Enum**:
- **Remote**: Changes from OneDrive
- **Local**: Changes from local filesystem

**ProcessingStatus Enum**:
- **New**: Initial state
- **Validated**: Passed validation
- **Conflicted**: Has conflicts
- **Error**: Processing failed
- **Completed**: Successfully processed

## Repositories

### DriveItemWithFuseRepository
**File**: `persistency/drive_item_with_fuse_repository.rs`

Core data access layer:

**Key Methods**:
- `get_drive_item_with_fuse_by_virtual_ino(ino: u64)`: Find by virtual inode
- `get_drive_item_with_fuse_by_virtual_path(path: &str)`: Find by virtual path
- `get_drive_item_with_fuse_by_onedrive_id(id: &str)`: Find by OneDrive ID
- `store_drive_item_with_fuse(item: &DriveItemWithFuse)`: Store new item
- `update_drive_item_with_fuse(item: &DriveItemWithFuse)`: Update existing item
- `delete_drive_item_with_fuse_by_onedrive_id(id: &str)`: Delete by OneDrive ID

### CachedDriveItemWithFuseRepository
**File**: `persistency/cached_drive_item_with_fuse_repository.rs`

Caching wrapper with TTL-based invalidation:

**Features**:
- Configurable TTL (default: 5 minutes)
- Automatic cache invalidation
- Memory-efficient caching

### ProcessingItemRepository
**File**: `persistency/processing_item_repository.rs`

Change processing queue management:

**Key Methods**:
- `get_unprocessed_items_by_change_type(change_type: &ChangeType)`: Get items by type
- `get_next_unprocessed_item_by_change_type(change_type: &ChangeType)`: Get next item
- `update_status_by_id(id: i64, status: &ProcessingStatus)`: Update status
- `update_validation_errors_by_id(id: i64, errors: &[String])`: Update errors
- `hause_keeping()`: Clean up completed/old items

### DownloadQueueRepository
**File**: `persistency/download_queue_repository.rs`

Download queue management:

**Key Methods**:
- `add_download_item(item: &DownloadQueueItem)`: Add to queue
- `get_next_download_item()`: Get next item for processing
- `update_status_by_id(id: i64, status: &str)`: Update status
- `remove_completed_items()`: Clean up completed downloads

### ProfileRepository
**File**: `persistency/profile_repository.rs`

User profile storage:

**Key Methods**:
- `store_profile(profile: &UserProfile)`: Store user profile
- `get_profile()`: Retrieve stored profile

### SyncStateRepository
**File**: `persistency/sync_state_repository.rs`

Synchronization state tracking:

**Key Methods**:
- `store_sync_state(state: &SyncState)`: Store sync state
- `get_latest_sync_state()`: Get most recent state

## Database Schema

### Tables Overview
- **drive_items_with_fuse**: Core data storage
- **processing_items**: Change tracking
- **download_queue**: Download management
- **profiles**: User profiles
- **sync_states**: Sync state history
- **tokens**: Authentication tokens

### Key Relationships
- **DriveItemWithFuse** ↔ **ProcessingItem**: Via OneDrive ID
- **DriveItemWithFuse** ↔ **DownloadQueueItem**: Via OneDrive ID
- **ProcessingItem** ↔ **SyncState**: Via timestamps

## Data Flow

```
OneDrive API → DriveItem → DriveItemWithFuse → FUSE Filesystem
     ↓              ↓            ↓              ↓
ProcessingItem → DownloadQueue → Local Files → Database
```

## Usage Patterns

### Reading Data
1. Query by virtual inode (FUSE operations)
2. Query by virtual path (file operations)
3. Query by OneDrive ID (sync operations)

### Writing Data
1. Store new items from OneDrive API
2. Update existing items with local changes
3. Queue downloads for file content

### Change Tracking
1. Create ProcessingItem for detected changes
2. Process items in priority order (Remote → Local)
3. Update status based on processing results
