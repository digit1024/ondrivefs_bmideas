# DBus Integration

## Overview

The DBus integration provides system-level communication, enabling the OneDrive daemon to interact with other applications and system services.

## Architecture

### DbusServerManager

**File**: `dbus_server/mod.rs`

Main server lifecycle manager:

```rust
pub struct DbusServerManager {
    app_state: Arc<AppState>,
    connection: Option<zbus::Connection>,
}
```

**Key Responsibilities**:

- Server startup and shutdown
- Connection management
- Service registration
- Signal broadcasting

## Service Implementation

### ServiceImpl

**File**: `dbus_server/server.rs`

Implements the DBus interface:

```rust
pub struct ServiceImpl {
    app_state: Arc<AppState>,
}
```

**Interface**: `org.freedesktop.OneDriveSync`

**Key Methods**:

- `get_status()`: Retrieve daemon status
- `start_sync()`: Manually trigger synchronization
- `pause_sync()`: Pause synchronization
- `resume_sync()`: Resume synchronization
- `get_conflicts()`: Get list of conflicts
- `resolve_conflict(conflict_id: String, resolution: String)`: Resolve specific conflict

## DBus Interface Definition

### DaemonStatus

**File**: `lib/dbus/types.rs`

```rust
pub struct DaemonStatus {
    pub is_authenticated: bool,      // Authentication status
    pub is_connected: bool,          // Network connectivity
    pub sync_status: SyncStatus,     // Current sync state
    pub has_conflicts: bool,         // Conflict presence
    pub is_mounted: bool,            // FUSE mount status
}
```

### SyncStatus Enum

```rust
pub enum SyncStatus {
    Running,    // Synchronization active
    Paused,     // Synchronization paused
    Error,      // Synchronization error
}
```

## Signal Broadcasting

### StatusBroadcastTask

**File**: `tasks/status_broadcast.rs`

Periodic status updates:

```rust
pub struct StatusBroadcastTask {
    app_state: Arc<AppState>,
    connection: zbus::Connection,
}
```

**Broadcast Signals**:

- **change-detected**: Emitted every 10 seconds with current status
- **sync-started**: Synchronization cycle begins
- **sync-completed**: Synchronization cycle completes
- **conflict-detected**: New conflict identified
- **error-occurred**: Error condition detected

### Signal Emission

```rust
impl StatusBroadcastTask {
    pub async fn run(&self) {
        loop {
            // Compute current status
            let status = compute_status(&self.app_state).await;
            
            // Emit status signal
            let _ = self.emit_status_signal(&status).await;
            
            // Wait for next broadcast
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }
}
```

## Message Handling

### MessageHandler

**File**: `dbus_server/message_handler.rs`

Routes DBus messages to appropriate handlers:

```rust
pub struct MessageHandler {
    app_state: Arc<AppState>,
}
```

**Message Types**:

- **Method Calls**: Interface method invocations
- **Property Access**: Property get/set operations
- **Signal Handling**: Incoming signal processing

## Integration Points

### AppState Integration

**File**: `dbus_server/mod.rs`

```rust
async fn compute_status(app_state: &Arc<AppState>) -> DaemonStatus {
    let is_authenticated = app_state.auth().get_valid_token().await.is_ok();
    let is_connected = matches!(
        app_state.connectivity().check_connectivity().await,
        crate::connectivity::ConnectivityStatus::Online
    );
    let sync_status = if let Some(metrics) = app_state.scheduler().get_task_metrics("sync_cycle").await {
        if metrics.is_running { SyncStatus::Running } else { SyncStatus::Paused }
    } else { SyncStatus::Paused };
    let has_conflicts = app_state.persistency().processing_item_repository()
        .get_processing_items_by_status(&ProcessingStatus::Conflicted)
        .await.map(|items| !items.is_empty()).unwrap_or(false);
    
    // ... mount status checking
    
    DaemonStatus { is_authenticated, is_connected, sync_status, has_conflicts, is_mounted }
}
```

### Connectivity Integration

**File**: `connectivity.rs`

Network status monitoring:

- **Online**: Full connectivity to OneDrive
- **Offline**: No network access
- **Limited**: Partial connectivity

### Scheduler Integration

**File**: `scheduler/periodic_scheduler.rs`

Task status monitoring:

- **Running**: Task currently executing
- **Paused**: Task suspended
- **Completed**: Task finished successfully
- **Error**: Task failed

## System Integration

### Desktop Integration

**File**: `resources/open-onedrive-daemon.desktop`

Desktop entry for system integration:

```ini
[Desktop Entry]
Name=Open OneDrive Daemon
Comment=OneDrive synchronization daemon
Exec=onedrive-daemon
Type=Application
Categories=Network;FileManager;
```

### Service Integration

**File**: `resources/open-onedrive-daemon.service`

Systemd service definition:

```ini
[Unit]
Description=Open OneDrive Daemon
After=network.target

[Service]
Type=simple
User=%i
ExecStart=/usr/bin/onedrive-daemon
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
```

## Error Handling

### Connection Failures

- **Automatic Retry**: Connection attempts retried automatically
- **Graceful Degradation**: Continue operation without DBus when possible
- **Error Logging**: Comprehensive error tracking

### Service Failures

- **Health Monitoring**: Regular health checks
- **Recovery Mechanisms**: Automatic service restart
- **User Notification**: Error reporting via alternative channels

## Performance Considerations

### Signal Frequency

- **Status Updates**: 10-second intervals (configurable)
- **Event-driven**: Immediate signals for important events
- **Batch Updates**: Grouped status updates when possible

### Resource Management

- **Connection Pooling**: Efficient DBus connection handling
- **Memory Usage**: Minimal memory footprint for signals
- **CPU Impact**: Low-overhead status computation

## Security

### Access Control

- **Session Bus**: User-level access only
- **Method Validation**: Input parameter validation
- **Permission Checks**: Operation authorization

### Data Privacy

- **Minimal Exposure**: Only necessary status information
- **No Sensitive Data**: No authentication tokens or file content
- **Audit Logging**: Access logging for debugging

## Debugging & Monitoring

### DBus Monitoring

```bash
# Monitor DBus messages
dbus-monitor --session "interface='org.freedesktop.OneDriveSync'"

# List available services
dbus-send --session --dest=org.freedesktop.DBus --type=method_call \
    /org/freedesktop/DBus org.freedesktop.DBus.ListNames
```

### Signal Testing

```bash
# Test status retrieval
dbus-send --session --dest=org.freedesktop.OneDriveSync \
    /org/freedesktop/OneDriveSync org.freedesktop.OneDriveSync.get_status
```

### Logging

- **DBus Operations**: All DBus interactions logged
- **Signal Emission**: Signal broadcast tracking
- **Error Conditions**: Detailed error reporting

## Future Enhancements

### Planned Features

- **Configuration Interface**: Runtime configuration changes
- **Statistics Interface**: Detailed sync statistics
- **Notification Interface**: User notification management
- **Plugin Interface**: Extensible functionality

### Integration Opportunities

- **GNOME Integration**: GNOME Shell integration
- **KDE Integration**: KDE Plasma integration
- **System Monitor**: System monitoring tools integration
- **Backup Tools**: Backup software integration
