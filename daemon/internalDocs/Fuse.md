# FUSE Implementation

## Overview

The FUSE (Filesystem in Userspace) implementation provides a virtual filesystem that maps OneDrive items to local paths, enabling seamless access to cloud files.

## Core Architecture

### OneDriveFuse

**File**: `fuse/filesystem.rs`

Main filesystem implementation:

```rust
pub struct OneDriveFuse {
    drive_item_with_fuse_repo: Arc<CachedDriveItemWithFuseRepository>,
    file_manager: Arc<DefaultFileManager>,
    app_state: Arc<AppState>,
    file_handle_manager: FileHandleManager,
    file_operations_manager: FileOperationsManager,
    database_manager: DatabaseManager,
}
```

**Key Responsibilities**:

- Filesystem initialization
- Delegation to specialized managers
- Root directory management

## Manager Classes

### FileHandleManager

**File**: `fuse/file_handles.rs`

Manages open file handles and file descriptors:

**Key Features**:

- **File Handle Tracking**: Maps FUSE file handles to local file descriptors
- **Concurrent Access**: Handles multiple open files
- **Resource Cleanup**: Automatic cleanup of closed handles

**Key Methods**:

- `open_file(path: &str, flags: i32)`: Open file and return handle
- `read_file(handle: u64, offset: i64, size: u32)`: Read from file
- `write_file(handle: u64, offset: i64, data: &[u8])`: Write to file
- `close_file(handle: u64)`: Close file handle

### FileOperationsManager

**File**: `fuse/file_operations.rs`

Handles file system operations:

**Key Methods**:

- `create_file(path: &str, mode: u32)`: Create new file
- `mkdir(path: &str, mode: u32)`: Create directory
- `unlink(path: &str)`: Delete file
- `rmdir(path: &str)`: Remove directory
- `rename(old_path: &str, new_path: &str)`: Move/rename file

### DriveItemManager

**File**: `fuse/drive_item_manager.rs`

Manages OneDrive drive items within FUSE context:

**Key Methods**:

- `get_drive_item_by_path(path: &str)`: Find item by path
- `create_drive_item(path: &str, is_dir: bool)`: Create new item
- `update_drive_item(item: &DriveItemWithFuse)`: Update existing item
- `delete_drive_item(path: &str)`: Delete item

### DatabaseManager

**File**: `fuse/database.rs`

FUSE-specific database operations:

**Key Methods**:

- `get_item_by_ino(ino: u64)`: Find item by inode
- `get_children_by_parent_ino(parent_ino: u64)`: Get directory contents
- `update_item_metadata(item: &DriveItemWithFuse)`: Update metadata

## File Handle (FH) & Inode (INO) Handling

### Inode Management

**File**: `fuse/attributes.rs`

Virtual inode system:

```rust
pub struct InodeAttributes {
    pub ino: u64,                        // Virtual inode number
    pub size: u64,                       // File size
    pub blocks: u64,                     // Block count
    pub atime: SystemTime,               // Access time
    pub mtime: SystemTime,               // Modification time
    pub ctime: SystemTime,               // Creation time
    pub mode: u32,                       // File permissions
    pub nlink: u32,                      // Link count
    pub uid: u32,                        // User ID
    pub gid: u32,                        // Group ID
    pub rdev: u32,                       // Device ID
}
```

**Inode Assignment**:

- **Root**: Always inode 1
- **Files**: Sequential assignment starting from 2
- **Directories**: Sequential assignment for directory structure
- **Virtual Paths**: Mapped to inodes via database

### File Handle Management

**File**: `fuse/file_handles.rs`

File handle lifecycle:

```rust
pub struct FileHandle {
    pub handle: u64,                     // FUSE file handle
    pub local_fd: Option<i32>,           // Local file descriptor
    pub path: String,                    // File path
    pub flags: i32,                      // Open flags
    pub created_at: SystemTime,          // Handle creation time
}
```

**Handle States**:

- **Open**: File is actively being accessed
- **Cached**: Handle cached for performance
- **Closed**: Handle released, resources cleaned up

## FUSE Operations Implementation

### Core Operations

**File**: `fuse/operations.rs`

Implements required FUSE operations:

**Directory Operations**:

- `lookup(parent: u64, name: &OsStr)`: Find file/directory
- `readdir(dir: u64, offset: i64, reply: &mut ReplyDirectory)`: Read directory contents
- `mkdir(parent: u64, name: &OsStr, mode: u32, reply: &mut ReplyEntry)`: Create directory
- `rmdir(parent: u64, name: &OsStr, reply: &mut ReplyEmpty)`: Remove directory

**File Operations**:

- `create(parent: u64, name: &OsStr, mode: u32, flags: u32, reply: &mut ReplyCreate)`: Create file
- `open(ino: u64, flags: i32, reply: &mut ReplyOpen)`: Open file
- `read(ino: u64, fh: u64, offset: i64, size: u32, reply: &mut ReplyData)`: Read file data
- `write(ino: u64, fh: u64, offset: i64, data: &[u8], write_flags: u32, reply: &mut ReplyWrite)`: Write file data
- `unlink(parent: u64, name: &OsStr, reply: &mut ReplyEmpty)`: Delete file

**Metadata Operations**:

- `getattr(ino: u64, reply: &mut ReplyAttr)`: Get file attributes
- `setattr(ino: u64, attr: SetAttr, reply: &mut ReplyAttr)`: Set file attributes
- `access(ino: u64, mask: u32, reply: &mut ReplyEmpty)`: Check file access

### Utility Functions

**File**: `fuse/utils.rs`

Helper functions for FUSE operations:

- `sync_await(future: impl Future<Output = Result<T>>)`: Synchronous await for async operations
- `convert_timestamp(timestamp: &str)`: Convert string timestamps to SystemTime

## Virtual Path System

### Path Resolution

**File**: `persistency/types.rs`

Virtual path computation:

```rust
impl DriveItemWithFuse {
    pub fn compute_virtual_path(&self) -> String {
        if let Some(parent_ref) = &self.drive_item.parent_reference {
            if let Some(parent_path) = &parent_ref.path {
                let mut path = parent_path.replace("/drive/root:", "");
                if !path.starts_with('/') {
                    path = format!("/{}", path);
                }
                if path == "/" {
                    format!("/{}", self.drive_item.name.as_deref().unwrap_or(""))
                } else {
                    format!("{}/{}", path, self.drive_item.name.as_deref().unwrap_or(""))
                }
            } else {
                format!("/{}", self.drive_item.name.as_deref().unwrap_or(""))
            }
        } else {
            format!("/{}", self.drive_item.name.as_deref().unwrap_or(""))
        }
    }
}
```

**Path Structure**:

- **Root**: `/`
- **Files**: `/path/to/file.txt`
- **Directories**: `/path/to/directory/`

## Performance Optimizations

### Caching Strategy

**File**: `persistency/cached_drive_item_with_fuse_repository.rs`

- **TTL-based Caching**: 5-minute default TTL
- **Memory Efficiency**: Automatic cache invalidation
- **Query Optimization**: Cached repository queries

### File Handle Reuse

- **Handle Pooling**: Reuse file handles when possible
- **Lazy Loading**: Load file content on demand
- **Background Sync**: Non-blocking file operations

## Error Handling

### FUSE Error Codes

- **ENOENT**: File not found
- **EACCES**: Permission denied
- **ENOSPC**: No space left
- **EIO**: Input/output error

### Recovery Mechanisms

- **Automatic Retry**: Failed operations retried automatically
- **Fallback Paths**: Alternative access methods
- **Error Logging**: Comprehensive error tracking

## Integration Points

### Database Integration

- **DriveItemWithFuse**: Core data model
- **ProcessingItem**: Change tracking
- **DownloadQueue**: File content management

### File System Integration

- **DefaultFileManager**: Local file operations
- **Download Management**: Background file downloads
- **Change Detection**: File system monitoring

### Synchronization Integration

- **SyncProcessor**: Background sync operations
- **Conflict Resolution**: Change conflict handling
- **Status Updates**: Real-time status reporting
