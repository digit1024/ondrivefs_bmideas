# Sync Process Decision Flow Diagrams

## Remote Changes Sync Flow

```mermaid
graph TD
    A[Start Remote Sync] --> B[Detect Conflicts]
    B --> C{Any Conflicts?}
    C -->|Yes| D[Attempt Auto-Resolve]
    D --> E{Auto-Resolve Successful?}
    E -->|Yes| F[Mark as Validated]
    E -->|No| G[Mark as Conflicted]
    C -->|No| F
    F --> H[Process Operation]
    H --> I{Operation Type}
    I -->|Create| J[Handle Remote Create]
    I -->|Update| K[Handle Remote Update]
    I -->|Delete| L[Handle Remote Delete]
    I -->|Move| M[Handle Remote Move]
    I -->|Rename| N[Handle Remote Rename]
    J --> O[Setup FUSE Metadata]
    J --> P{Should Download?}
    P -->|Yes| Q[Add to Download Queue]
    P -->|No| R[Skip Download]
    K --> S[Check ETag Change]
    S -->|Changed| T[Add to Download Queue]
    S -->|No Change| U[Skip Download]
    L --> V[Remove from Download Queue]
    L --> W[Delete Local File]
    L --> X[Delete Child Items]
    M --> Y[Move on OneDrive]
    M --> Z[Update FUSE Metadata]
    N --> AA[Update FUSE Metadata]
    Q --> AB[Mark as Done]
    R --> AB
    T --> AB
    U --> AB
    V --> AB
    W --> AB
    X --> AB
    Y --> AB
    Z --> AB
    AA --> AB
    G --> AC[End - Conflicted]
```

## Local Changes Sync Flow

```mermaid
graph TD
    A[Start Local Sync] --> B[Squash Local Changes]
    B --> C[Apply Squashing Rules]
    C --> D[Create+Delete = Remove All]
    C --> E[Create+Modify = Final Create]
    C --> F[Multiple Modifies = Last Modify]
    C --> G[Multiple Renames = Last Rename]
    C --> H[Multiple Moves = Last Move]
    D --> I[Detect Conflicts]
    E --> I
    F --> I
    G --> I
    H --> I
    I --> J{Any Conflicts?}
    J -->|Yes| K[Mark as Conflicted]
    J -->|No| L[Mark as Validated]
    L --> M[Process Operation]
    M --> N{Operation Type}
    N -->|Create| O[Handle Local Create]
    N -->|Update| P[Handle Local Update]
    N -->|Delete| Q[Handle Local Delete]
    N -->|Move| R[Handle Local Move]
    N -->|Rename| S[Handle Local Rename]
    O --> T{Is Folder?}
    T -->|Yes| U[Create Folder on OneDrive]
    T -->|No| V[Upload File to OneDrive]
    U --> W[Update DB with Real ID]
    V --> W
    P --> X{Is Folder?}
    X -->|Yes| Y[Update Metadata Only]
    X -->|No| Z[Upload Updated File]
    Q --> AA[Delete from OneDrive]
    R --> AB[Move on OneDrive]
    S --> AC[Rename on OneDrive]
    W --> AD[Mark as Done]
    Y --> AD
    Z --> AD
    AA --> AD
    AB --> AD
    AC --> AD
    K --> AE[End - Conflicted]
```

## Key Sync Process Details

### Priority-Based Processing
- **Remote changes** are processed first (sync_processor.rs:41-54)
- **Local changes** are processed after remote changes are handled

### Conflict Detection
- Uses **ctag/etag comparison** for content conflicts
- **Parent state checks** (deleted parent detection)
- **Name collision detection** in target directories
- **Operation type matching** (Create vs Create, Update vs Delete, etc.)

### Auto-Resolution Capabilities
- Attempts to **restore deleted parents** from OneDrive
- Handles **metadata-only changes** automatically
- Resolves **parent chain recreation** for moved items

### Local Change Squashing Rules
1. **Create + Delete sequence** = Remove all processing items
2. **Create + Modifications** = Final Create operation
3. **Multiple Modifies** = Keep only last Modify
4. **Multiple Renames** = Keep only last Rename
5. **Multiple Moves** = Keep only last Move

### File Download Logic
- **Folders**: Downloaded on demand when accessed
- **Files**: Downloaded based on configured download folders
- **ETag changes**: Trigger re-download of modified files
- **Parent-based filtering**: Only downloads files in specified folders

### Database Operations
- **Temporary ID management**: Local files get temporary IDs until synced
- **ID propagation**: Updates all references when temporary IDs become real OneDrive IDs
- **Parent-child relationships**: Maintains proper parent inode references
- **Sync status tracking**: Tracks sync state for each item

### Error Handling
- **Conflicted items**: Marked for manual resolution
- **Processing errors**: Retry mechanism with error status
- **Parent recreation**: Automatic folder structure restoration
- **Download queue management**: Proper cleanup of deleted items