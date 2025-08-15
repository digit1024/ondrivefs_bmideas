# Useful Commands & Development Tools

## Overview

This document provides essential commands and tools for development, debugging, and database operations with the OneDrive daemon.

## Database Operations (SQLite)

### Database Location
```bash
# Default database location
~/.local/share/onedrive-sync/onedrive_sync.db

# Check if database exists
ls -la ~/.local/share/onedrive-sync/

# View database size
du -h ~/.local/share/onedrive-sync/onedrive_sync.db
```

### SQLite CLI Access
```bash
# Connect to database
sqlite3 ~/.local/share/onedrive-sync/onedrive_sync.db

# Exit SQLite
.exit
.quit
```

### Core Data Queries

#### DriveItemWithFuse Queries
```sql
-- View all drive items
SELECT * FROM drive_items_with_fuse LIMIT 10;

-- Find item by virtual inode
SELECT * FROM drive_items_with_fuse WHERE virtual_ino = 1;

-- Find item by virtual path
SELECT * FROM drive_items_with_fuse WHERE virtual_path = '/Documents/file.txt';

-- Find item by OneDrive ID (escape exclamation marks!)
SELECT * FROM drive_items_with_fuse WHERE onedrive_id = '12345!67890';

-- Count total items
SELECT COUNT(*) FROM drive_items_with_fuse;

-- View items by file source
SELECT file_source, COUNT(*) FROM drive_items_with_fuse GROUP BY file_source;
```

#### ProcessingItem Queries
```sql
-- View all processing items
SELECT * FROM processing_items LIMIT 10;

-- Find items by status
SELECT * FROM processing_items WHERE status = 'New';
SELECT * FROM processing_items WHERE status = 'Conflicted';

-- Find items by change type
SELECT * FROM processing_items WHERE change_type = 'Remote';
SELECT * FROM processing_items WHERE change_type = 'Local';

-- Count items by status
SELECT status, COUNT(*) FROM processing_items GROUP BY status;

-- Find recent items
SELECT * FROM processing_items ORDER BY created_at DESC LIMIT 20;
```

#### DownloadQueue Queries
```sql
-- View download queue
SELECT * FROM download_queue LIMIT 10;

-- Find items by status
SELECT * FROM download_queue WHERE status = 'Pending';
SELECT * FROM download_queue WHERE status = 'Downloading';

-- Count by status
SELECT status, COUNT(*) FROM download_queue GROUP BY status;

-- Find high priority items
SELECT * FROM download_queue WHERE priority > 5 ORDER BY priority DESC;
```

#### Profile & Sync State
```sql
-- View user profile
SELECT * FROM profiles LIMIT 1;

-- View sync states
SELECT * FROM sync_states ORDER BY created_at DESC LIMIT 5;

-- View latest sync state
SELECT * FROM sync_states ORDER BY created_at DESC LIMIT 1;
```

### Database Schema Inspection
```sql
-- List all tables
.tables

-- View table schema
.schema drive_items_with_fuse
.schema processing_items
.schema download_queue

-- List table columns
PRAGMA table_info(drive_items_with_fuse);
PRAGMA table_info(processing_items);

-- Check database integrity
PRAGMA integrity_check;

-- View database statistics
PRAGMA stats;
```

### Advanced Queries

#### Join Operations
```sql
-- Join drive items with processing items
SELECT 
    d.name, 
    d.virtual_path, 
    p.status, 
    p.change_type
FROM drive_items_with_fuse d
LEFT JOIN processing_items p ON d.onedrive_id = p.onedrive_id
WHERE p.status = 'Conflicted';

-- Find items with download queue entries
SELECT 
    d.name, 
    d.virtual_path, 
    dq.status as download_status,
    dq.priority
FROM drive_items_with_fuse d
JOIN download_queue dq ON d.onedrive_id = dq.onedrive_id;
```

#### Data Analysis
```sql
-- File type distribution
SELECT 
    CASE 
        WHEN d.folder IS NOT NULL THEN 'Directory'
        WHEN d.file IS NOT NULL THEN 'File'
        ELSE 'Unknown'
    END as item_type,
    COUNT(*) as count
FROM drive_items_with_fuse d
GROUP BY item_type;

-- Sync status overview
SELECT 
    d.file_source,
    COUNT(*) as total_items,
    SUM(CASE WHEN p.status = 'Completed' THEN 1 ELSE 0 END) as synced,
    SUM(CASE WHEN p.status = 'Conflicted' THEN 1 ELSE 0 END) as conflicts
FROM drive_items_with_fuse d
LEFT JOIN processing_items p ON d.onedrive_id = p.onedrive_id
GROUP BY d.file_source;
```

## File System Operations

### FUSE Mount Status
```bash
# Check if FUSE is mounted
mount | grep OneDrive

# Check mount point
ls -la ~/OneDrive/

# Check mount options
findmnt ~/OneDrive

# Check FUSE process
ps aux | grep onedrive
ps aux | grep fuse
```

### File System Checks
```bash
# Check file permissions
ls -la ~/OneDrive/

# Check directory structure
tree ~/OneDrive/ -L 3

# Find specific files
find ~/OneDrive/ -name "*.txt" -type f
find ~/OneDrive/ -type d -name "Documents"

# Check file sizes
du -sh ~/OneDrive/*

# Check file timestamps
stat ~/OneDrive/Documents/file.txt
```

### FUSE Debug Information
```bash
# Check FUSE kernel module
lsmod | grep fuse

# Check FUSE version
fusermount --version

# Check FUSE mount options
cat /proc/mounts | grep OneDrive

# Check FUSE statistics (if available)
cat /proc/fs/fuse/stats
```

### File Operation Testing
```bash
# Test file creation
echo "test content" > ~/OneDrive/test_file.txt

# Test directory creation
mkdir ~/OneDrive/test_directory

# Test file reading
cat ~/OneDrive/test_file.txt

# Test file deletion
rm ~/OneDrive/test_file.txt

# Test directory removal
rmdir ~/OneDrive/test_directory
```

## Daemon Operations

### Building & Compilation
```bash
# Navigate to daemon directory
cd daemon/

# Check compilation without building
cargo check

# Build in debug mode
cargo build

# Build in release mode
cargo build --release

# Clean build artifacts
cargo clean

# Check for unused dependencies
cargo check --message-format=short

# Run clippy (code quality checks)
cargo clippy

# Run tests
cargo test

# Run specific test
cargo test test_name
```

### Running the Daemon
```bash
# Run in debug mode
cargo run

# Run with specific features
cargo run --features debug

# Run with environment variables
RUST_LOG=debug cargo run

# Run with custom configuration
RUST_LOG=info RUST_BACKTRACE=1 cargo run

# Run in background
nohup cargo run > daemon.log 2>&1 &

# Check if daemon is running
ps aux | grep "cargo run"
pgrep -f "onedrive"
```

### Daemon Startup & Monitoring
```bash
# Wait for daemon to start (check logs)
tail -f daemon.log

# Check daemon status via DBus
dbus-send --session --dest=org.freedesktop.OneDriveSync \
    /org/freedesktop/OneDriveSync org.freedesktop.OneDriveSync.get_status

# Monitor DBus messages
dbus-monitor --session "interface='org.freedesktop.OneDriveSync'"

# Check daemon logs
tail -f ~/.local/share/onedrive-sync/logs/daemon.log

# Check system logs
journalctl -u onedrive-daemon -f
```

### Daemon Control
```bash
# Stop daemon (if running in foreground)
Ctrl+C

# Kill daemon process
pkill -f "onedrive"
pkill -f "cargo run"

# Force unmount FUSE
fusermount -u ~/OneDrive

# Check daemon exit status
echo $?
```

## Development & Debugging

### Logging Configuration
```bash
# Set log level
export RUST_LOG=debug

# Set specific module logging
export RUST_LOG=onedrive_daemon=debug,fuse=info

# Enable backtraces
export RUST_BACKTRACE=1

# Set log output
export RUST_LOG_STYLE=always
```

### Performance Monitoring
```bash
# Monitor CPU usage
top -p $(pgrep -f "onedrive")

# Monitor memory usage
ps aux | grep onedrive | awk '{print $6}'

# Monitor file descriptors
lsof -p $(pgrep -f "onedrive")

# Monitor network connections
netstat -tulpn | grep onedrive
ss -tulpn | grep onedrive
```

### Debug Tools
```bash
# Check system resources
free -h
df -h
iostat 1

# Check network connectivity
ping graph.microsoft.com
curl -I https://graph.microsoft.com/v1.0

# Check SSL certificates
openssl s_client -connect graph.microsoft.com:443

# Check DNS resolution
nslookup graph.microsoft.com
dig graph.microsoft.com
```

### Testing & Validation
```bash
# Run integration tests
cargo test --test integration_tests

# Run specific test module
cargo test --test processing_item_tests

# Run tests with output
cargo test -- --nocapture

# Run tests in parallel
cargo test --jobs 4

# Run tests with specific features
cargo test --features test_utils
```

## Troubleshooting Commands

### Common Issues
```bash
# Check if port 8080 is in use (OAuth callback)
netstat -tulpn | grep :8080
lsof -i :8080

# Check file permissions
ls -la ~/.local/share/onedrive-sync/
ls -la ~/OneDrive/

# Check disk space
df -h ~/.local/share/onedrive-sync/
df -h ~/OneDrive/

# Check inotify limits
cat /proc/sys/fs/inotify/max_user_watches
cat /proc/sys/fs/inotify/max_user_instances
```

### Recovery Commands
```bash
# Reset database (⚠️ DESTRUCTIVE)
rm ~/.local/share/onedrive-sync/onedrive_sync.db

# Clear cache
rm -rf ~/.local/share/onedrive-sync/cache/

# Reset authentication
rm ~/.local/share/onedrive-sync/auth.json

# Force unmount and cleanup
fusermount -u ~/OneDrive 2>/dev/null
rm -rf ~/OneDrive/*

# Restart daemon
pkill -f "onedrive"
sleep 2
cd daemon && cargo run
```

## Environment Setup

### Development Environment
```bash
# Install Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Install development dependencies
sudo apt install sqlite3 fuse libfuse-dev pkg-config

# Install additional tools
sudo apt install tree htop iotop iftop

# Set up development aliases
echo 'alias onedrive-build="cd daemon && cargo build"' >> ~/.bashrc
echo 'alias onedrive-run="cd daemon && cargo run"' >> ~/.bashrc
echo 'alias onedrive-check="cd daemon && cargo check"' >> ~/.bashrc
```

### Production Environment
```bash
# Install system dependencies
sudo apt install fuse libfuse2

# Create system user
sudo useradd -r -s /bin/false onedrive

# Set up systemd service
sudo cp resources/open-onedrive-daemon.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable onedrive-daemon
sudo systemctl start onedrive-daemon
```

## Quick Reference

### Daily Development
```bash
cd daemon
cargo check          # Quick compilation check
cargo build          # Build for testing
cargo test           # Run tests
cargo run            # Run daemon
```

### Database Debugging
```bash
sqlite3 ~/.local/share/onedrive-sync/onedrive_sync.db
.tables              # List tables
.schema table_name   # View schema
SELECT * FROM table_name LIMIT 5;  # Quick data check
```

### System Monitoring
```bash
ps aux | grep onedrive    # Check process
mount | grep OneDrive     # Check mount
ls -la ~/OneDrive/       # Check files
tail -f daemon.log       # Monitor logs
```

### Emergency Recovery
```bash
fusermount -u ~/OneDrive     # Unmount FUSE
pkill -f onedrive            # Kill daemon
rm -rf ~/OneDrive/*          # Clear mount point
cd daemon && cargo run       # Restart
```
