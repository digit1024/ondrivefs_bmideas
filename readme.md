# OneDrive Sync

A desktop application to access, modify, and sync content from OneDrive, running as a background daemon on Linux.

## Features
- OneDrive file access and sync
- Runs as a background daemon
- Secure token storage (with system keyring)
- PKCE OAuth2 authentication (no client secret required)

## Requirements
- Rust toolchain
- **keyutils** package (for secure keyring access on Linux)
  - Install with: `sudo apt install keyutils`
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
cargo run -- [--daemon] [--auth] [--list] [--local-dir <PATH>] [--remote-dir <PATH>] [--interval <SECONDS>]
```

- `--daemon`: Run the app as a background daemon for continuous sync
- `--auth`: Run the authorization flow only
- `--list`: List files in OneDrive root
- `--local-dir <PATH>`: Local directory to sync (default: ./sync)
- `--remote-dir <PATH>`: Remote OneDrive directory (default: /sync)
- `--interval <SECONDS>`: Sync interval in seconds (default: 300)

## Notes
- The first run will prompt you to log in and authorize the app in your browser.
- Tokens are stored securely using your system keyring (requires `keyutils` on Linux).
- No client secret is required; the app uses the PKCE flow for secure authentication.

---

*Work in progress.*
