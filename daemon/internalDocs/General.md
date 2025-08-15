# OneDrive Daemon - Architecture Overview

## Project Structure

The OneDrive daemon is a FUSE filesystem implementation that provides local access to OneDrive files with background synchronization.

## Core Components

### 🔧 **AppState** (`app_state.rs`)
Central state container holding all shared components:
- **ProjectConfig**: Application configuration and directories
- **PersistencyManager**: Database and persistence layer
- **ConnectivityChecker**: Network connectivity monitoring
- **OneDriveClient**: OneDrive API client
- **OneDriveAuth**: Authentication manager
- **DefaultFileManager**: File system operations
- **PeriodicScheduler**: Task scheduling

### 🗄️ **Persistency** (`persistency/`)
Database layer with SQLite backend:
- **DriveItemWithFuseRepository**: Core data model combining OneDrive items with FUSE metadata
- **ProcessingItemRepository**: Change tracking and processing queue
- **DownloadQueueRepository**: File download management
- **ProfileRepository**: User profile storage
- **SyncStateRepository**: Synchronization state tracking

### 🔄 **Sync** (`sync/`)
Synchronization engine:
- **SyncProcessor**: Main sync orchestration
- **SyncStrategy**: Conflict detection and resolution
- **ConflictResolution**: Conflict handling strategies

### 📁 **FUSE** (`fuse/`)
Filesystem implementation:
- **OneDriveFuse**: Main FUSE filesystem
- **FileHandleManager**: File handle management
- **FileOperationsManager**: File operation handling
- **DriveItemManager**: Drive item operations
- **DatabaseManager**: FUSE-specific database operations

### 🌐 **OneDrive Service** (`onedrive_service/`)
API integration:
- **OneDriveClient**: HTTP client for OneDrive Graph API
- **HttpClient**: HTTP request handling
- **OneDriveModels**: API data structures

### 🔐 **Authentication** (`auth/`)
OAuth2 authentication flow:
- **OneDriveAuth**: OAuth2 implementation with PKCE
- **TokenStore**: Token persistence and refresh

### 📡 **DBus Integration** (`dbus_server/`)
System integration:
- **DbusServerManager**: DBus server lifecycle
- **ServiceImpl**: DBus interface implementation
- **MessageHandler**: DBus message routing

### ⏰ **Scheduler** (`scheduler/`)
Task management:
- **PeriodicScheduler**: Periodic task execution
- **PeriodicTask**: Individual task definition
- **TaskMetrics**: Performance monitoring

### 📋 **Tasks** (`tasks/`)
Background operations:
- **DeltaUpdate**: OneDrive delta synchronization
- **StatusBroadcast**: Status updates via DBus

## Data Flow

```
OneDrive API ←→ OneDriveClient ←→ SyncProcessor ←→ ProcessingQueue
                    ↓                    ↓              ↓
              DriveItemWithFuse ←→ FUSE Filesystem ←→ Local Files
                    ↓                    ↓              ↓
              Database Storage ←→ FileManager ←→ DownloadQueue
```

## Key Concepts

- **DriveItemWithFuse**: Core data model combining OneDrive metadata with FUSE filesystem data
- **ProcessingItem**: Change tracking for sync operations
- **Virtual Paths**: FUSE filesystem paths mapped to OneDrive hierarchy
- **Delta Sync**: Incremental synchronization using OneDrive delta API
- **Conflict Resolution**: Automatic and manual conflict handling

## Related Documentation

- [Database Models](DBModel.md) - Data structures and repositories
- [FUSE Implementation](Fuse.md) - Filesystem implementation details
- [Synchronization](Sync.md) - Sync process and conflict resolution
- [DBus Integration](Dbus.md) - System integration interface
- [OneDrive API](OneDriveApi.md) - API integration details
- [Authentication](Authentication.md) - OAuth2 authentication flow
- [Additional Components](Additional.md) - Supporting systems and utilities
- [Useful Commands](UsefulCommands.md) - Development commands and tools
