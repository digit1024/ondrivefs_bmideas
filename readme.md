# OneDrive Sync

A desktop application to access, modify, and sync content from OneDrive, running as a background daemon on Linux.

## Features
- OneDrive file access and sync
- Runs as a background daemon
- Secure token storage (planned)
- PKCE OAuth2 authentication (planned)

## Usage

```sh
cargo run -- [--daemon]
```

- `--daemon`: Run the app as a background daemon for continuous sync

## Setup
1. Register an Azure app (see Microsoft documentation)
2. Configure the app with your client ID and redirect URI
3. Run the app to authorize and start syncing

---

*Work in progress.*
