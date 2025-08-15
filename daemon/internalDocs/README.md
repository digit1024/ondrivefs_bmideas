# OneDrive Daemon - Internal Documentation

## Overview

This directory contains comprehensive internal documentation for the OneDrive daemon module. This documentation is designed for developers working on the project and provides detailed technical information about the system architecture, implementation details, and internal workings.

## Documentation Structure

### 📋 **General.md**
**Project Architecture Overview**
- Complete system architecture
- Core component descriptions
- Data flow diagrams
- Key concepts and terminology
- Links to specific sections

**Use Case**: Start here to understand the overall system design and how components interact.

### 🗄️ **DBModel.md**
**Database Models & Repositories**
- Core data structures (DriveItemWithFuse, ProcessingItem, etc.)
- Repository implementations and methods
- Database schema overview
- Data relationships and flow patterns

**Use Case**: Understand the data layer, database structure, and how data flows through the system.

### 📁 **Fuse.md**
**FUSE Implementation Details**
- Filesystem architecture and components
- File handle and inode management
- FUSE operations implementation
- Virtual path system
- Performance optimizations

**Use Case**: Work with the filesystem layer, understand FUSE operations, or debug filesystem issues.

### 🔄 **Sync.md**
**Synchronization System**
- Sync process flow and architecture
- Conflict detection and resolution
- Delta synchronization
- Processing queue management
- Performance optimizations

**Use Case**: Work on synchronization logic, conflict resolution, or understand the sync workflow.

### 📡 **Dbus.md**
**DBus Integration**
- System integration architecture
- Service implementation details
- Signal broadcasting
- Message handling
- System integration points

**Use Case**: Work with system integration, DBus communication, or desktop environment integration.

### 🌐 **OneDriveApi.md**
**OneDrive API Integration**
- API client implementation
- Data models and structures
- API operations and endpoints
- Authentication integration
- Error handling and retry logic

**Use Case**: Work with OneDrive API integration, modify API calls, or understand API data flow.

### 🔐 **Authentication.md**
**OAuth2 Authentication System**
- OAuth2 flow implementation
- PKCE security features
- Token management and refresh
- Security considerations
- Integration points

**Use Case**: Work with authentication, modify OAuth flow, or understand security implementation.

### 🔧 **Additional.md**
**Supporting Components & Utilities**
- Message broker system
- File management operations
- Connectivity monitoring
- Logging and monitoring
- Scheduler and task management
- Integration patterns and security

**Use Case**: Understand supporting systems, utilities, and architectural patterns used throughout the system.

### 🛠️ **UsefulCommands.md**
**Development Commands & Tools**
- SQLite database queries and operations
- FUSE filesystem debugging commands
- Daemon building, running, and monitoring
- Development and troubleshooting tools
- Environment setup and configuration

**Use Case**: Quick reference for development commands, database operations, and system debugging.

## How to Use This Documentation

### For New Developers
1. **Start with General.md** - Get the big picture
2. **Read relevant sections** based on your task
3. **Use code references** to find specific implementations
4. **Follow data flow** to understand system behavior

### For Code Analysis
1. **Identify the component** you're working with
2. **Read the relevant documentation** section
3. **Use file references** to locate source code
4. **Follow the data flow** to understand interactions

### For Debugging
1. **Identify the problem area** (sync, FUSE, auth, etc.)
2. **Read the relevant documentation** for context
3. **Use the error handling sections** to understand recovery
4. **Follow the integration points** to trace issues

### For Feature Development
1. **Understand the current architecture** from General.md
2. **Identify affected components** and read their documentation
3. **Follow the data flow** to ensure consistency
4. **Update relevant documentation** as you make changes

## Code References

Each documentation file includes:
- **File paths** to source code
- **Function signatures** and key methods
- **Data structure definitions**
- **Configuration constants**
- **Integration points**

## Data Flow Patterns

### Synchronization Flow
```
OneDrive API → OneDriveClient → SyncProcessor → ProcessingQueue → FUSE Filesystem
```

### Authentication Flow
```
User → Browser → Microsoft OAuth → Local Server → Token Exchange → Storage
```

### File Operations Flow
```
FUSE Request → FileHandleManager → FileOperationsManager → OneDriveClient → API
```

## Key Concepts

### DriveItemWithFuse
The core data model that combines OneDrive metadata with FUSE filesystem data.

### ProcessingItem
Change tracking entity for synchronization operations.

### Virtual Paths
FUSE filesystem paths mapped to OneDrive hierarchy.

### Delta Sync
Incremental synchronization using OneDrive delta API.

### Conflict Resolution
Automatic and manual conflict handling strategies.

## Development Guidelines

### When Adding Features
1. **Update relevant documentation** sections
2. **Maintain data flow consistency**
3. **Document new integration points**
4. **Update error handling sections**

### When Modifying Architecture
1. **Update General.md** with new structure
2. **Update affected component documentation**
3. **Maintain cross-references**
4. **Update data flow diagrams**

### When Debugging
1. **Use documentation** to understand context
2. **Follow data flow** to trace issues
3. **Check integration points** for problems
4. **Update documentation** with findings

## Maintenance

### Documentation Updates
- **Keep code references current** when files move
- **Update data flow diagrams** when architecture changes
- **Maintain cross-references** between sections
- **Add new sections** for new major components

### Code Synchronization
- **Verify file paths** are still correct
- **Check function signatures** match current code
- **Update examples** when APIs change
- **Maintain consistency** with actual implementation

## Contributing

When contributing to this documentation:
1. **Follow the existing structure** and format
2. **Include code references** to source files
3. **Provide concrete examples** where possible
4. **Maintain cross-references** between sections
5. **Update the README** if adding new sections

## Quick Reference

### Core Files
- **main.rs**: Application entry point and lifecycle
- **app_state.rs**: Central state management
- **lib.rs**: Module exports

### Key Directories
- **persistency/**: Database layer and repositories
- **fuse/**: Filesystem implementation
- **sync/**: Synchronization engine
- **auth/**: Authentication system
- **onedrive_service/**: API integration
- **dbus_server/**: System integration
- **tasks/**: Background operations
- **scheduler/**: Task management

### Common Patterns
- **Repository Pattern**: Data access abstraction
- **Manager Pattern**: Component coordination
- **Strategy Pattern**: Pluggable algorithms
- **Observer Pattern**: Event notification
