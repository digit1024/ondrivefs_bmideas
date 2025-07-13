# OneDrive Sync - MIME Type Handler Installation

This directory contains the necessary files to register the `application/onedrivedownload` MIME type and set up the `open-onedrive` handler.

## 📁 Files

- `open-onedrive.desktop` - Desktop entry for the application
- `onedrive-sync.xml` - MIME type definition
- `install.sh` - Installation script
- `uninstall.sh` - Uninstallation script

## 🚀 Installation

### Prerequisites

1. Build the project:
   ```bash
   cargo build
   ```

2. Run the installation script:
   ```bash
   ./resources/programfiles/install.sh
   ```

### What the installation does:

1. **Creates symlink**: `~/.local/bin/open-onedrive` → `target/debug/onedrive-sync-daemon`
2. **Installs desktop file**: `~/.local/share/applications/open-onedrive.desktop`
3. **Registers MIME type**: `application/onedrivedownload`
4. **Updates databases**: Desktop and MIME type databases

## 🗑️ Uninstallation

To remove the MIME type handler:

```bash
./resources/programfiles/uninstall.sh
```

## 🔧 How it works

1. **FUSE filesystem** returns files with MIME type `application/onedrivedownload`
2. **Desktop environment** recognizes the MIME type
3. **Your application** (`open-onedrive`) is launched with the file path
4. **Your app** queues the download and exits

## 🧪 Testing

1. Mount the FUSE filesystem:
   ```bash
   cargo run -- --mount /tmp/onedrive
   ```

2. Try to open a remote file:
   ```bash
   xdg-open /tmp/onedrive/some-remote-file.txt
   ```

3. Your application should be launched and queue the download.

## 📋 MIME Type Details

- **Type**: `application/onedrivedownload`
- **Description**: OneDrive Download File
- **Handler**: `open-onedrive %f`
- **Pattern**: `*.onedrivedownload` (optional)

## 🔍 Troubleshooting

### Check MIME type registration:
```bash
xdg-mime query default application/onedrivedownload
```

### Check if symlink exists:
```bash
ls -la ~/.local/bin/open-onedrive
```

### Check desktop file:
```bash
cat ~/.local/share/applications/open-onedrive.desktop
```

### Check MIME definition:
```bash
cat ~/.local/share/mime/packages/onedrive-sync.xml
``` 