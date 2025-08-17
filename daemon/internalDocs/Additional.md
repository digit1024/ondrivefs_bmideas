# Additional Components

## Overview

This document covers additional important components that provide supporting functionality to the main system.

## Message Broker

### MessageBroker
**File**: `message_broker.rs`

Inter-component communication system:

```rust
pub struct MessageBroker {
    subscribers: HashMap<MessageType, Vec<Box<dyn MessageHandler>>>,
}
```

**Key Features**:
- **Event Publishing**: Broadcast messages to subscribers
- **Type-based Routing**: Route messages by type
- **Async Support**: Non-blocking message delivery
- **Component Decoupling**: Loose coupling between components

**Message Types**:
- **Sync Events**: Synchronization status updates
- **File Events**: File operation notifications
- **Error Events**: Error condition notifications
- **Status Events**: System status changes

## File Management

### DefaultFileManager
**File**: `file_manager.rs`

Local filesystem operations:

```rust
pub struct DefaultFileManager {
    project_config: Arc<ProjectConfig>,
    download_dir: PathBuf,
}
```

**Key Responsibilities**:
- **File Operations**: Create, read, write, delete files
- **Directory Management**: Create and manage directories
- **Path Resolution**: Convert virtual paths to local paths
- **Download Management**: Handle file downloads
- **Change Detection**: Monitor local filesystem changes

**Key Methods**:
- `create_file(path: &Path, content: &[u8])`: Create new file
- `read_file(path: &Path)`: Read file content
- `write_file(path: &Path, content: &[u8])`: Write file content
- `delete_file(path: &Path)`: Remove file
- `ensure_directory(path: &Path)`: Create directory if needed

## Connectivity Management

### ConnectivityChecker
**File**: `connectivity.rs`

Network connectivity monitoring:

```rust
pub struct ConnectivityChecker {
    check_urls: Vec<String>,
    timeout: Duration,
}
```

**Connectivity Status**:
```rust
pub enum ConnectivityStatus {
    Online,     // Full connectivity
    Limited,    // Partial connectivity
    Offline,    // No connectivity
}
```

**Key Features**:
- **Multi-URL Checking**: Check multiple endpoints
- **Timeout Handling**: Configurable timeout settings
- **Status Caching**: Cache connectivity status
- **Automatic Retry**: Retry failed checks

**Check Endpoints**:
- Microsoft Graph API
- OneDrive service endpoints
- General internet connectivity

## Logging System

### LogAppender
**File**: `log_appender.rs`

Structured logging implementation:

```rust
pub struct LogAppender {
    log_dir: PathBuf,
    max_file_size: u64,
    max_files: usize,
}
```

**Logging Features**:
- **File Rotation**: Automatic log file rotation
- **Size Limits**: Configurable file size limits
- **Retention Policy**: Keep specified number of files
- **Structured Format**: JSON-formatted log entries
- **Performance Logging**: Track operation performance

**Log Levels**:
- **Error**: Error conditions
- **Warn**: Warning conditions
- **Info**: General information
- **Debug**: Debug information
- **Trace**: Detailed tracing

## Scheduler System

### PeriodicScheduler
**File**: `scheduler/periodic_scheduler.rs`

Task scheduling and execution:

```rust
pub struct PeriodicScheduler {
    tasks: HashMap<String, PeriodicTask>,
    running: bool,
}
```

**Task Management**:
- **Periodic Execution**: Execute tasks at specified intervals
- **Task Metrics**: Track task performance and status
- **Dynamic Scheduling**: Add/remove tasks at runtime
- **Error Handling**: Handle task failures gracefully

**Task Types**:
- **Sync Cycle**: OneDrive synchronization
- **Status Broadcast**: DBus status updates
- **Cleanup Tasks**: System maintenance operations
- **Health Checks**: System health monitoring

### PeriodicTask
**File**: `scheduler/periodic_scheduler.rs`

Individual task definition:

```rust
pub struct PeriodicTask {
    pub name: String,
    pub interval: Duration,
    pub metrics: TaskMetrics,
    pub task: Box<dyn Fn() -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync>,
}
```

**Task Features**:
- **Configurable Intervals**: Set execution frequency
- **Performance Metrics**: Track execution statistics
- **Async Support**: Non-blocking task execution
- **Error Recovery**: Handle task failures

## Task System

### DeltaUpdate
**File**: `tasks/delta_update.rs`

OneDrive delta synchronization:

```rust
pub struct SyncCycle {
    app_state: Arc<AppState>,
    processing_repo: ProcessingItemRepository,
    drive_item_with_fuse_repo: DriveItemWithFuseRepository,
}
```

**Sync Features**:
- **Delta API Integration**: Use OneDrive delta API
- **Change Processing**: Process remote changes
- **State Management**: Track synchronization state
- **Error Recovery**: Handle sync failures

### StatusBroadcast
**File**: `tasks/status_broadcast.rs`

DBus status broadcasting:

```rust
pub struct StatusBroadcastTask {
    app_state: Arc<AppState>,
    connection: zbus::Connection,
}
```

**Broadcast Features**:
- **Periodic Updates**: Regular status broadcasts
- **Event-driven**: Immediate broadcasts for important events
- **Status Computation**: Real-time status calculation
- **DBus Integration**: System integration via DBus

## Integration Patterns

### Repository Pattern
Used throughout the persistency layer:
- **Abstract Data Access**: Hide database implementation details
- **Type Safety**: Strongly typed data access
- **Caching Support**: Built-in caching capabilities
- **Transaction Support**: Database transaction handling

### Manager Pattern
Used for component coordination:
- **Single Responsibility**: Each manager handles one aspect
- **Resource Management**: Manage component lifecycle
- **Error Handling**: Centralized error handling
- **Configuration**: Component-specific configuration

### Strategy Pattern
Used in synchronization:
- **Conflict Detection**: Single strategy for detecting conflicts
- **Manual Resolution**: All conflicts require user intervention
- **Consistent Behavior**: Predictable conflict handling
- **User Control**: Users have full control over conflict resolution

## Configuration Management

### ProjectConfig
**File**: `lib/config.rs`

Application configuration:

```rust
pub struct ProjectConfig {
    pub project_dirs: ProjectDirs,
    pub sync_interval: Duration,
    pub max_retries: u32,
    pub timeout: Duration,
}
```

**Configuration Sources**:
- **Environment Variables**: Runtime configuration
- **Configuration Files**: Persistent settings
- **Default Values**: Sensible defaults
- **User Preferences**: User-specific settings

## Error Handling

### Error Types
- **Network Errors**: Connection and timeout issues
- **API Errors**: OneDrive API failures
- **File System Errors**: Local filesystem issues
- **Database Errors**: Database operation failures
- **Authentication Errors**: OAuth and token issues

### Error Recovery
- **Automatic Retry**: Retry failed operations
- **Fallback Strategies**: Alternative approaches
- **User Notification**: Inform users of issues
- **Logging**: Comprehensive error logging

## Performance Considerations

### Caching Strategy
- **Repository Caching**: Cache database queries
- **File Content Caching**: Cache file content
- **API Response Caching**: Cache API responses
- **TTL-based Invalidation**: Automatic cache refresh

### Resource Management
- **Connection Pooling**: Reuse HTTP connections
- **Memory Management**: Efficient memory usage
- **File Handle Management**: Optimize file operations
- **Background Processing**: Non-blocking operations

## Monitoring & Observability

### Metrics Collection
- **Task Performance**: Track task execution times
- **API Performance**: Monitor API response times
- **File Operations**: Track filesystem performance
- **Error Rates**: Monitor failure patterns

### Health Checks
- **Connectivity**: Network connectivity status
- **Authentication**: Token validity and refresh
- **Database**: Database health and performance
- **Filesystem**: FUSE mount status and health

## Security Considerations

### Data Protection
- **Token Encryption**: Secure token storage
- **File Permissions**: Restrict file access
- **Network Security**: HTTPS-only communication
- **Input Validation**: Validate all inputs

### Access Control
- **User Isolation**: User-specific data separation
- **Permission Checks**: Verify operation permissions
- **Audit Logging**: Track access and operations
- **Secure Communication**: Encrypted data transmission
