# OneDrive Sync 🚀

A desktop application to access, modify, and sync content from OneDrive, running as a background daemon on Linux. Because who doesn't love having their files in the cloud AND locally? It's like having your cake and eating it too, but with better error handling! 🍰

## Features ✨
- **OneDrive file access and sync** - Because manually downloading files is so 2010
- **FUSE filesystem mount** - Mount OneDrive as a local filesystem (because why not?)
- **Runs as a background daemon** - Set it and forget it (until something breaks)
- **Secure token storage** - Using your system keyring (because we're not savages)
- **PKCE OAuth2 authentication** - No client secret required (because who needs more secrets?)

## Requirements 🛠️
- **Rust toolchain** - Because C++ is so last decade
- **keyutils** package (for secure keyring access on Linux)
  - Install with: `sudo apt install keyutils`
- **FUSE** support (usually available by default on Linux)
  - Install with: `sudo apt install fuse`
- A registered Microsoft Entra ID (Azure AD) application
- **Patience** - Because cloud APIs are like cats: sometimes they work, sometimes they don't

## Entra ID (Azure AD) Setup 🔧
1. Go to the [Azure Portal](https://portal.azure.com/) (try not to get lost in the UI maze)
2. Navigate to **Microsoft Entra ID** (or **Azure Active Directory**) > **App registrations** > **New registration**
3. Fill in:
   - **Name:** Any name (e.g., `onedrive-sync`)
   - **Supported account types:** Accounts in any organizational directory and personal Microsoft accounts
   - **Redirect URI:** Public client/native, `http://localhost:8080/callback`
4. Click **Register** (and hope it works on the first try)
5. Go to **API permissions** > **Add a permission** > **Microsoft Graph** > **Delegated permissions**
   - Add: `Files.ReadWrite`, `offline_access`
6. Copy your **Application (client) ID** and set it in your code (`src/auth/onedrive_auth.rs`)

## Usage 🎯

```sh
cargo run -- [--daemon] [--auth] [--list] [--local-dir <PATH>] [--remote-dir <PATH>] [--interval <SECONDS>] [--mount] [--mount-point <PATH>]
```

### Basic Commands 🎮
- `--daemon`: Run the app as a background daemon for continuous sync
- `--auth`: Run the authorization flow only (because tokens expire, just like milk)
- `--list`: List files in OneDrive root
- `--local-dir <PATH>`: Local directory to sync (default: ./sync)
- `--remote-dir <PATH>`: Remote OneDrive directory (default: /sync)
- `--interval <SECONDS>`: Sync interval in seconds (default: 300)

### FUSE Mount Commands 🔌
- `--mount`: Mount OneDrive as a FUSE filesystem
- `--mount-point <PATH>`: Specify mount point (default: ~/OneDrive)

### File Operations 📁
- `--list-dir <PATH>`: List files in a specific OneDrive directory
- `--get-file <REMOTE_PATH> <LOCAL_PATH>`: Download a file from OneDrive
- `--put-file <LOCAL_PATH> <REMOTE_PATH>`: Upload a local file to OneDrive

### Settings Management ⚙️
- `--settings-add-folder-to-sync <FOLDER>`: Add a folder to sync list
- `--settings-remove-folder-to-sync <FOLDER>`: Remove a folder from sync list
- `--settings-list-folders-to-sync`: List all folders set to sync

## FUSE Mount Usage 🔗

### Mount OneDrive
```sh
# Mount with default settings
cargo run -- --mount

# Mount at custom location
cargo run -- --mount --mount-point /mnt/onedrive

# Mount and run in background (because we're fancy)
nohup cargo run -- --mount > onedrive-mount.log 2>&1 &
```

### How FUSE Mount Works 🔍
1. **Mount Point**: OneDrive is mounted at `~/OneDrive` by default
2. **Cache Directory**: Files are cached in `~/.onedrive/cache/` (because we're not downloading the same file twice)
3. **Temporary Files**: Modified files are stored in `~/.onedrive/cache/tmp/`
4. **Change Queue**: File modifications are queued for upload during sync
5. **Sync Integration**: Uses existing sync capabilities to upload changes

### File Operations 📄
- **Read**: Files are downloaded from OneDrive to cache on first access
- **Write**: Changes are written to temporary files and queued for upload
- **Sync**: Background sync process uploads queued changes to OneDrive
- **Cleanup**: Temporary files are cleaned up after successful upload

### Unmounting 🔌
- Press `Ctrl+C` to gracefully unmount the filesystem
- The application will clean up temporary files and flush metadata

## Recent Refactoring 🏗️

We've been busy making this codebase less of a mess! Here's what we've done:

### Code Organization 📂
- **Separated HTTP client** from OneDrive client for better separation of concerns
- **Broke down large methods** into smaller, more focused ones
- **Added proper error handling** throughout the codebase
- **Cleaned up unused code** (but kept it for future features)

### Architecture Improvements 🏛️
- **HTTP Client Module**: Handles all HTTP operations with Microsoft Graph API
- **OneDrive Client**: Focuses on OneDrive-specific business logic
- **Better Error Context**: More descriptive error messages
- **Improved Testing**: Unit tests for URL construction and edge cases

### Future-Proofing 🚀
- **Write Operations**: Framework in place for file uploads and modifications
- **Move Operations**: Infrastructure ready for file/folder moves
- **Change Tracking**: Queue system for tracking local modifications
- **Extensible Design**: Easy to add new OneDrive API features

## Notes 📝
- The first run will prompt you to log in and authorize the app in your browser.
- Tokens are stored securely using your system keyring (requires `keyutils` on Linux).
- No client secret is required; the app uses the PKCE flow for secure authentication.
- FUSE mount requires appropriate permissions and FUSE support on your system.
- The mount is read-only for now; write operations are queued for background sync.

## Build Requirements 🛠️
**POP OS 24.04 (Alpha 7)** - Because we like living on the edge:
```bash
sudo apt install build-essential pkg-config libssl-dev libfuse-dev fuse keyutils ca-certificates curl
```

## Contributing 🤝
Found a bug? Want to add a feature? Don't be shy! Just remember:
- Write tests (because we're not animals)
- Follow the existing code style (because consistency is key)
- Add humor to your commit messages (because life is too short for boring commits)

---

*Work in progress, but getting better every day! 🎉*

*P.S. If this doesn't work, try turning it off and on again. It works for everything else! 🔄*
