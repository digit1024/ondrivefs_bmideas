# Synchronization System

## Overview

The synchronization system provides bidirectional synchronization between OneDrive and local filesystem, handling conflicts and maintaining consistency.

## Core Components

### SyncProcessor
**File**: `sync/sync_processor.rs`

Main synchronization orchestrator:

```rust
pub struct SyncProcessor {
    strategy: SyncStrategy,
    app_state: Arc<AppState>,
    processing_repo: ProcessingItemRepository,
    drive_item_with_fuse_repo: DriveItemWithFuseRepository,
}
```

**Key Responsibilities**:
- Process remote and local changes
- Coordinate conflict resolution
- Manage processing queue
- Handle validation errors

## Synchronization Process Flow

### 1. Change Detection
**File**: `tasks/delta_update.rs`

#### Remote Changes (OneDrive → Local)
```rust
pub async fn get_delta_changes(&self) -> Result<Vec<DriveItem>> {
    let sync_state_repo = SyncStateRepository::new(self.app_state.persistency().pool().clone());
    let sync_state = sync_state_repo.get_latest_sync_state().await?;
    
    // Get delta changes from OneDrive API
    let changes = self.app_state.onedrive().get_delta_changes(&sync_state.delta_link).await?;
    
    // Process each change
    for change in changes {
        self.process_delta_change(change).await?;
    }
    
    Ok(changes)
}
```

**Change Types**:
- **Create**: New file/folder created
- **Update**: Existing item modified
- **Delete**: Item removed
- **Move**: Item relocated

#### Local Changes (Local → OneDrive)
**File**: `file_manager.rs`

Detected through:
- FUSE file operations
- File system monitoring
- Manual change detection

### 2. Processing Queue Management
**File**: `persistency/processing_item_repository.rs`

#### ProcessingItem States
```rust
pub enum ProcessingStatus {
    New,           // Initial state
    Validated,     // Passed validation
    Conflicted,    // Has conflicts
    Error,         // Processing failed
    Completed,     // Successfully processed
}
```

#### Priority Processing
**File**: `sync/sync_processor.rs`

```rust
pub async fn process_all_items(&self) -> Result<()> {
    // 1. Process Remote changes first
    let remote_items = self.processing_repo
        .get_unprocessed_items_by_change_type(&ChangeType::Remote)
        .await?;
    
    // 2. Process Local changes after remote changes
    let local_items = self.processing_repo
        .get_unprocessed_items_by_change_type(&ChangeType::Local)
        .await?;
}

// Auto-resolution happens during conflict detection:
pub async fn process_single_item(&self, item: &ProcessingItem) -> Result<()> {
    match item.change_type {
        ChangeType::Remote => {
            let mut conflicts = self.strategy.detect_remote_conflicts(item).await?;
            
            // Try to auto-resolve conflicts before marking item as conflicted
            if !conflicts.is_empty() {
                self.strategy.auto_resolve_remote_conflicts(item, &mut conflicts).await?;
            }
            
            if conflicts.is_empty() {
                // Process item normally
                self.process_remote_item(item).await?;
            } else {
                // Mark as conflicted for manual resolution
                self.mark_as_conflicted(item, &conflicts).await?;
            }
        }
    }
}
```

**Processing Order**:
1. **Remote Changes**: OneDrive → Local (highest priority)
2. **Local Changes**: Local → OneDrive (after remote completion)

### 3. Conflict Detection & Resolution
**File**: `sync/sync_strategy.rs`

#### Conflict Detection
```rust
pub async fn detect_remote_conflicts(&self, item: &ProcessingItem) -> Result<Vec<RemoteConflict>> {
    let mut conflicts = Vec::new();
    
    // 1. Parent folder state - check if parent was deleted locally
    if let Some(parent_ref) = &item.drive_item.parent_reference {
        if parent_item.is_deleted() {
            conflicts.push(RemoteConflict::ModifyOnParentDelete);
        }
    }
    
    // 2. Name collision - check if item with same name exists locally
    // 3. Content conflicts - check modification timestamps and etags
    // 4. Move/rename conflicts - check destination paths
    
    Ok(conflicts)
}

pub async fn detect_local_conflicts(&self, item: &ProcessingItem) -> Result<Vec<LocalConflict>> {
    // Similar logic for local → remote conflicts
}
```

#### Conflict Types
**File**: `sync/conflicts.rs`

The system defines two separate conflict enums for different conflict scenarios:

```rust
/// Remote conflicts (OneDrive → Local)
pub enum RemoteConflict {
    CreateOnCreate(String),        // Remote create, but local item exists with same name
    ModifyOnModify(String, String), // Both remote and local modified (local etag, remote etag)
    ModifyOnDelete,                // Remote modified, but local item deleted
    ModifyOnParentDelete,          // Remote modified, but local parent folder deleted
    DeleteOnModify,                // Remote deleted, but local item modified
    RenameOrMoveOnExisting,        // Remote renamed/moved, but target name exists locally
    MoveOnMove,                    // Remote moved, but local also moved to different location
    MoveToDeletedParent,           // Remote moved, but destination parent deleted locally
}

/// Local conflicts (Local → OneDrive)
pub enum LocalConflict {
    CreateOnExisting,              // Local create, but remote item exists with same name
    ModifyOnDeleted,               // Local modified, but remote item deleted
    ModifyOnModified,              // Local modified, but remote also modified
    DeleteOnModified,              // Local deleted, but remote item modified
    RenameOrMoveToExisting,        // Local renamed/moved, but target exists on server
    RenameOrMoveOfDeleted,         // Local renamed/moved, but source deleted from server
}
```

#### Resolution Strategy
**File**: `sync/conflict_resolution.rs`

**Auto-Resolution**:
- **ModifyOnParentDelete** and **MoveToDeletedParent** conflicts are automatically resolved by restoring the parent from OneDrive
- Parent restoration process:
  1. Fetch parent DriveItem from OneDrive by parent ID
  2. Mark DriveItemWithFuse as not deleted (restore or create)
  3. Remove processing errors from parent items
  4. Continue with normal processing

**Manual Resolution**:
- User intervention required for all other conflicts
- Conflict notification via DBus
- Manual conflict resolution UI allows users to choose:
  - Keep Local: Use the local version
  - Use Remote: Use the OneDrive version

### 4. File Operations
**File**: `file_manager.rs`

#### Download Management
```rust
pub async fn download_file(&self, item: &DriveItemWithFuse) -> Result<()> {
    // Add to download queue
    let download_item = DownloadQueueItem {
        onedrive_id: item.drive_item.id.clone(),
        local_path: self.get_local_path(item),
        priority: self.calculate_priority(item),
        // ... other fields
    };
    
    self.download_queue_repo.add_download_item(&download_item).await?;
    Ok(())
}
```

#### Upload Management
```rust
pub async fn upload_file(&self, local_path: &Path) -> Result<()> {
    // Create upload request
    let upload_request = self.create_upload_request(local_path).await?;
    
    // Execute upload
    let result = self.app_state.onedrive().upload_file(&upload_request).await?;
    
    // Update local database
    self.update_local_metadata(&result).await?;
    
    Ok(())
}
```

## Delta Synchronization

### Delta API Integration
**File**: `tasks/delta_update.rs`

#### Delta Link Management
```rust
pub async fn update_delta_link(&self, delta_link: &str) -> Result<()> {
    let sync_state = SyncState {
        delta_link: delta_link.to_string(),
        last_sync: chrono::Utc::now().to_rfc3339(),
    };
    
    self.sync_state_repo.store_sync_state(&sync_state).await?;
    Ok(())
}
```

**Benefits**:
- **Incremental Updates**: Only changed items processed
- **Efficiency**: Reduced API calls and data transfer
- **Consistency**: Maintains sync state across sessions

### Sync Cycle Execution
**File**: `tasks/delta_update.rs`

```rust
pub async fn run(&self) -> Result<()> {
    // 1. Get delta changes
    let changes = self.get_delta_changes().await?;
    
    // 2. Process each change
    for change in changes {
        self.process_delta_change(change).await?;
    }
    
    // 3. Update delta link
    if let Some(delta_link) = self.get_delta_link().await? {
        self.update_delta_link(&delta_link).await?;
    }
    
    Ok(())
}
```

## Performance Optimizations

### Batch Processing
- **Grouped Operations**: Process multiple items together
- **Parallel Processing**: Concurrent item processing
- **Queue Optimization**: Priority-based processing order

### Caching Strategy
- **Metadata Caching**: Cache frequently accessed metadata
- **Content Caching**: Cache file content for performance
- **TTL-based Invalidation**: Automatic cache refresh

### Resource Management
- **Connection Pooling**: Reuse HTTP connections
- **Memory Management**: Efficient memory usage
- **Background Processing**: Non-blocking operations

## Error Handling & Recovery

### Error Categories
1. **Network Errors**: Connection failures, timeouts
2. **API Errors**: OneDrive API failures
3. **File System Errors**: Permission, disk space issues
4. **Data Errors**: Corruption, validation failures

### Recovery Mechanisms
- **Automatic Retry**: Exponential backoff for transient errors
- **Fallback Strategies**: Alternative approaches when primary fails
- **Error Logging**: Comprehensive error tracking
- **User Notification**: Error reporting via DBus

### Monitoring & Metrics
**File**: `scheduler/periodic_scheduler.rs`

```rust
pub struct TaskMetrics {
    pub execution_count: usize,
    pub success_count: usize,
    pub error_count: usize,
    pub average_duration: Duration,
    pub last_execution: Option<SystemTime>,
}
```

## Integration Points

### Database Integration
- **ProcessingItem**: Change tracking and status management
- **DriveItemWithFuse**: Core data model
- **SyncState**: Synchronization state persistence

### FUSE Integration
- **File Operations**: Direct filesystem access
- **Change Detection**: Real-time change monitoring
- **Metadata Updates**: Filesystem metadata synchronization

### External Systems
- **OneDrive API**: Cloud storage integration
- **DBus**: System integration and notifications
- **File Manager**: Local filesystem operations

## Configuration

### Sync Intervals
- **Default**: 30 seconds
- **Configurable**: Via configuration file
- **Adaptive**: Based on system load and activity

### Conflict Resolution
- **Strategy**: Manual resolution only - all conflicts require user intervention
- **User Choice**: Manual conflict resolution via DBus interface
- **No Automatic Resolution**: Users must explicitly choose between local and remote versions

### Performance Tuning
- **Batch Size**: Number of items processed per cycle
- **Concurrency**: Parallel processing limits
- **Cache TTL**: Cache invalidation timing
