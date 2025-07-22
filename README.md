# OpenOneDrive

An experimental OneDrive sync client for Linux, featuring:
- FUSE-based virtual filesystem
- Two-way sync with conflict resolution
- Modern UI (libcosmic, Rust)
- notifications and panel applet (tbd)

- Designed for POP!_OS

---

## 🚀 Features
- **FUSE Filesystem:** Mount your OneDrive as a local drive
- **Two-way Sync:** Handles both remote and local changes
- **Conflict Resolution:** Smart strategies for file conflicts
- **Modern UI:** Built with libcosmic for a native look
- **Notifications:** System and in-app notifications
- **Autostart Daemon:** User-level systemd service for background sync
- **Flatpak & AppImage:** Easy distribution and installation

---

## 🏗️ Architecture

```mermaid
flowchart TD
    subgraph "Daemon (Rust)"
        A1["AppState<br/>config, db, auth, file mgr, scheduler"]
        A2["PersistencyManager<br/>SQLite, SQLx"]
        A3["OneDriveClient<br/>API, Auth"]
        A4["FileManager<br/>Local ops"]
        A5["FUSE Filesystem<br/>Virtual FS"]
        A6["SyncProcessor<br/>Two-way sync, conflict res."]
        A7["Scheduler<br/>Periodic tasks"]
        A8["DBusServer<br/>IPC"]
        A9["MessageBroker<br/>Internal events"]
    end
    subgraph "UI (Rust + libcosmic)"
        B1["AppModel<br/>State, nav, config"]
        B2["Pages<br/>Status, Folders, Queues, Logs"]
        B3["Notifications"]
        B4["DBusClient"]
    end
    subgraph "Shared"
        C1["onedrive-sync-lib"]
    end

    A1 -->|uses| A2
    A1 -->|uses| A3
    A1 -->|uses| A4
    A1 -->|uses| A5
    A1 -->|uses| A6
    A1 -->|uses| A7
    A1 -->|uses| A8
    A1 -->|uses| A9
    A3 -->|auth| A1
    A5 -->|mounts| UserFS
    A6 -->|calls| A2
    A6 -->|calls| A3
    A6 -->|calls| A4
    A8 -->|IPC| B4
    B1 -->|pages| B2
    B1 -->|notifies| B3
    B1 -->|IPC| B4
    B4 -->|calls| A8
    A1 -->|uses| C1
    B1 -->|uses| C1
```

## 🛠️ Installation

### 1. 
1. **Build:**
   ```sh
   cargo build 
   ```
2. **Install binaries and desktop files:**
   ```sh
   ./resources/programfiles/install.sh 
   ```
3. **Enable autostart daemon:**
   ```sh
   systemctl --user enable --now open-onedrive-daemon.service
   ```


## ⚡ Autostart Daemon 
1. **Copy the service file:**
   ```sh
   cp resources/programfiles/open-onedrive-daemon.service ~/.config/systemd/user/
   ```
2. **Enable and start:**
   ```sh
   systemctl --user enable --now open-onedrive-daemon.service
   ```

---

## 🖥️ Desktop Integration
- **UI:** Launch "OpenOneDrive UI" from your app menu.
- **Daemon:** Hidden from menu, runs in background for sync and notifications.
- **MIME Handler:** Handles `application/onedrivedownload` files for direct download.

---


## 📝 License
MIT

---

## 🙋 FAQ

- **Q: Where are files stored?**
  - A: Downloaded files are stored in a flat directory under `~/.local/share/onedrive-sync/downloads`.
- **What is the answer  to the Ultimate Question of Life, The Universe, and Everything?**
  - 42 
`  
---

## 🤝 Contributing
PRs and issues welcome! 


