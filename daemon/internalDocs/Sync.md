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
```

**Processing Order**:
1. **Remote Changes**: OneDrive → Local (highest priority)
2. **Local Changes**: Local → OneDrive (after remote completion)

### 3. Conflict Detection & Resolution
**File**: `sync/sync_strategy.rs`

#### Conflict Detection
```rust
pub async fn detect_remote_conflicts(&self, item: &ProcessingItem) -> Result<Vec<Conflict>> {
    let mut conflicts = Vec::new();
    
    // Check for file system conflicts
    if let Some(conflict) = self.check_filesystem_conflicts(item).await? {
        conflicts.push(conflict);
    }
    
    // Check for data conflicts
    if let Some(conflict) = self.check_data_conflicts(item).await? {
        conflicts.push(conflict);
    }
    
    Ok(conflicts)
}
```

#### Conflict Types
**File**: `sync/conflicts.rs`

```rust
pub enum ConflictType {
    FileExists,        // File already exists locally
    DirectoryExists,   // Directory already exists locally
    PermissionDenied,  // Insufficient permissions
    DiskFull,          // No disk space
    NetworkError,      // Network connectivity issues
    DataMismatch,      // Content differs between local and remote
}
```

#### Resolution Strategies
**File**: `sync/conflict_resolution.rs`

**Automatic Resolution**:
- **Remote Wins**: Use OneDrive version (default for remote changes)
- **Local Wins**: Use local version (default for local changes)
- **Merge**: Combine changes when possible

**Manual Resolution**:
- User intervention required
- Conflict notification via DBus
- Manual conflict resolution UI

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
- **Default Strategy**: Remote wins for remote changes, local wins for local changes
- **User Override**: Manual conflict resolution
- **Policy Configuration**: Configurable resolution rules

### Performance Tuning
- **Batch Size**: Number of items processed per cycle
- **Concurrency**: Parallel processing limits
- **Cache TTL**: Cache invalidation timing
