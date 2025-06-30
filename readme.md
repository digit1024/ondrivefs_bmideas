# OneDrive Sync

A desktop application to access, modify, and sync content from OneDrive, running as a background daemon on Linux.

## Features
- OneDrive file access and sync
- **FUSE filesystem mount** - Mount OneDrive as a local filesystem
- Runs as a background daemon
- Secure token storage (with system keyring)
- PKCE OAuth2 authentication (no client secret required)

## Requirements
- Rust toolchain
- **keyutils** package (for secure keyring access on Linux)
  - Install with: `sudo apt install keyutils`
- **FUSE** support (usually available by default on Linux)
  - Install with: `sudo apt install fuse`
- A registered Microsoft Entra ID (Azure AD) application

## Entra ID (Azure AD) Setup
1. Go to the [Azure Portal](https://portal.azure.com/)
2. Navigate to **Microsoft Entra ID** (or **Azure Active Directory**) > **App registrations** > **New registration**
3. Fill in:
   - **Name:** Any name (e.g., `onedrive-sync`)
   - **Supported account types:** Accounts in any organizational directory and personal Microsoft accounts
   - **Redirect URI:** Public client/native, `http://localhost:8080/callback`
4. Click **Register**
5. Go to **API permissions** > **Add a permission** > **Microsoft Graph** > **Delegated permissions**
   - Add: `Files.ReadWrite`, `offline_access`
6. Copy your **Application (client) ID** and set it in your code (`src/onedrive_auth.rs`)

## Usage

```sh
cargo run -- [--daemon] [--auth] [--list] [--local-dir <PATH>] [--remote-dir <PATH>] [--interval <SECONDS>] [--mount] [--mount-point <PATH>]
```

### Basic Commands
- `--daemon`: Run the app as a background daemon for continuous sync
- `--auth`: Run the authorization flow only
- `--list`: List files in OneDrive root
- `--local-dir <PATH>`: Local directory to sync (default: ./sync)
- `--remote-dir <PATH>`: Remote OneDrive directory (default: /sync)
- `--interval <SECONDS>`: Sync interval in seconds (default: 300)

### FUSE Mount Commands
- `--mount`: Mount OneDrive as a FUSE filesystem
- `--mount-point <PATH>`: Specify mount point (default: ~/OneDrive)

### File Operations
- `--list-dir <PATH>`: List files in a specific OneDrive directory
- `--get-file <REMOTE_PATH> <LOCAL_PATH>`: Download a file from OneDrive
- `--put-file <LOCAL_PATH> <REMOTE_PATH>`: Upload a local file to OneDrive

### Settings Management
- `--settings-add-folder-to-sync <FOLDER>`: Add a folder to sync list
- `--settings-remove-folder-to-sync <FOLDER>`: Remove a folder from sync list
- `--settings-list-folders-to-sync`: List all folders set to sync

## FUSE Mount Usage

### Mount OneDrive
```sh
# Mount with default settings
cargo run -- --mount

# Mount at custom location
cargo run -- --mount --mount-point /mnt/onedrive

# Mount and run in background
nohup cargo run -- --mount > onedrive-mount.log 2>&1 &
```

### How FUSE Mount Works
1. **Mount Point**: OneDrive is mounted at `~/OneDrive` by default
2. **Cache Directory**: Files are cached in `~/.onedrive/cache/`
3. **Temporary Files**: Modified files are stored in `~/.onedrive/cache/tmp/`
4. **Change Queue**: File modifications are queued for upload during sync
5. **Sync Integration**: Uses existing sync capabilities to upload changes

### File Operations
- **Read**: Files are downloaded from OneDrive to cache on first access
- **Write**: Changes are written to temporary files and queued for upload
- **Sync**: Background sync process uploads queued changes to OneDrive
- **Cleanup**: Temporary files are cleaned up after successful upload

### Unmounting
- Press `Ctrl+C` to gracefully unmount the filesystem
- The application will clean up temporary files and flush metadata

## Notes
- The first run will prompt you to log in and authorize the app in your browser.
- Tokens are stored securely using your system keyring (requires `keyutils` on Linux).
- No client secret is required; the app uses the PKCE flow for secure authentication.
- FUSE mount requires appropriate permissions and FUSE support on your system.
- The mount is read-only for now; write operations are queued for background sync.

---

*Work in progress.*

Build requirements POP OS 24.04 (Aplha 7)
sudo apt install build-essential pkg-config libssl-dev libfuse-dev fuse keyutils ca-certificates curl