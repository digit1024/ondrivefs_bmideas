//! OneDrive FUSE filesystem for Linux
//!
//! This is a FUSE filesystem that provides access to OneDrive files
//! through a local mount point. Files are cached locally and synchronized
//! with OneDrive in the background.

mod app_state;
mod auth;
mod connectivity;
mod dbus_server;
mod file_manager;
mod fuse;
mod log_appender;
mod message_broker;
mod onedrive_service;
mod persistency;
mod scheduler;
mod sync;
mod tasks;

use crate::app_state::{app_state_factory, AppState};
use crate::file_manager::{DefaultFileManager, FileManager};
use crate::fuse::OneDriveFuse;
use crate::log_appender::setup_logging;
use crate::persistency::download_queue_repository::DownloadQueueRepository;
use crate::persistency::profile_repository::ProfileRepository;
use crate::tasks::delta_update::SyncCycle;
use anyhow::{Context, Result};
use clap::Arg;
use clap::Command;
use fuser::MountOption;
use log::{error, info, warn};
use onedrive_sync_lib::notifications::{NotificationSender, NotificationUrgency};
use std::fs;
use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use std::thread;
use tokio::signal;
use tokio::sync::broadcast;
use tokio::time::{sleep, Duration};

// Add shutdown signal handling
use tokio::sync::oneshot;

/// Shutdown manager for graceful application termination
#[derive(Clone)]
struct ShutdownManager {
    shutdown_tx: broadcast::Sender<()>,
}

impl ShutdownManager {
    fn new() -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self { shutdown_tx }
    }

    fn subscribe(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }

    async fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }
}

/// Application configuration and setup
struct AppSetup {
    app_state: Arc<crate::app_state::AppState>,
}

impl AppSetup {
    /// Initialize the application with all required components
    async fn initialize() -> Result<Self> {
        info!("🚀 Initializing OneDrive FUSE daemon...");

        // Initialize project configuration
        let app_state = app_state_factory()
            .await
            .context("Failed to initialize application state")?;

        // Setup logging
        let log_dir = app_state.config().project_dirs.data_dir().to_path_buf();
        setup_logging(&log_dir)
            .await
            .context("Failed to setup logging")?;

        info!("✅ Application state initialized successfully");
        Ok(Self {
            app_state: Arc::new(app_state),
        })
    }

    /// Authenticate with OneDrive
    async fn authenticate(&self) -> Result<()> {
        info!("🔐 Starting authentication process...");

        let auth = self.app_state.auth();

        // Try to load existing tokens
        match auth.load_tokens() {
            Ok(_) => {
                info!("✅ Existing tokens loaded successfully");
                Ok(())
            }
            Err(_) => {
                info!("🔑 No valid tokens found, starting authorization flow...");
                auth.authorize().await.context("Authorization failed")?;

                auth.load_tokens()
                    .context("Failed to load tokens after authorization")?;

                info!("✅ Authentication completed successfully");
                Ok(())
            }
        }
    }

    /// Initialize database and verify connectivity
    async fn setup_infrastructure(&self) -> Result<()> {
        info!("🗄️ Initializing database and connectivity...");

        // Initialize database schema
        self.app_state
            .persistency()
            .init_database()
            .await
            .context("Failed to initialize database schema")?;

        // Verify connectivity
        let connectivity_status = self.app_state.connectivity().check_connectivity().await;
        info!("📡 Connectivity status: {}", connectivity_status);

        if connectivity_status == crate::connectivity::ConnectivityStatus::Offline {
            return Err(anyhow::anyhow!("No internet connectivity available"));
        }

        info!("✅ Infrastructure setup completed");
        Ok(())
    }

    /// Initialize user profile
    async fn setup_user_profile(&self) -> Result<()> {
        info!("👤 Setting up user profile...");

        let profile_repo = ProfileRepository::new(self.app_state.persistency().pool().clone());

        // Try to get existing profile
        match profile_repo.get_profile().await {
            Ok(Some(profile)) => {
                info!(
                    "✅ Found stored profile: {} ({})",
                    profile.display_name.as_deref().unwrap_or("Unknown"),
                    profile.mail.as_deref().unwrap_or("No email")
                );
            }
            Ok(None) => {
                info!("📋 No stored profile found, fetching from API...");
                self.fetch_and_store_profile(&profile_repo).await?;
            }
            Err(e) => {
                error!("⚠️ Error retrieving stored profile: {}", e);
                info!("🔄 Attempting to fetch fresh profile...");
                self.fetch_and_store_profile(&profile_repo).await?;
            }
        }

        Ok(())
    }

    /// Fetch and store user profile from OneDrive API
    async fn fetch_and_store_profile(&self, profile_repo: &ProfileRepository) -> Result<()> {
        let profile = self
            .app_state
            .onedrive()
            .get_user_profile()
            .await
            .context("Failed to get user profile")?;

        profile_repo
            .store_profile(&profile)
            .await
            .context("Failed to store profile")?;

        info!(
            "✅ Profile fetched and stored: {} ({})",
            profile.display_name.as_deref().unwrap_or("Unknown"),
            profile.mail.as_deref().unwrap_or("No email")
        );

        Ok(())
    }

    /// Start the main sync cycle
    #[allow(dead_code)]
    async fn start_sync_cycle(&self) -> Result<()> {
        info!("🔄 Starting sync cycle...");

        let sync_cycle = SyncCycle::new(self.app_state.clone());
        sync_cycle.run().await.context("Sync cycle failed")?;

        info!("✅ Sync cycle completed successfully");
        Ok(())
    }

    /// Display application information
    fn display_info(&self) {
        info!("📊 Application Information:");
        info!(
            "   Database location: {}",
            self.app_state.persistency().db_path().display()
        );
        info!(
            "   Data directory: {}",
            self.app_state.config().project_dirs.data_dir().display()
        );
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let matches = Command::new("OneDrive Client for Linux by digit1024@github")
        .version("01.0")
        .about("Mount OneDrive as a FUSE filesystem or handle OneDrive files")
        .arg(
            Arg::new("file")
                .num_args(1)
                .help("File path to handle (for MIME type handler)"),
        )
        .arg(
            Arg::new("readdirplus")
                .long("readdirplus")
                .help("Advertise readdirplus support to the kernel"),
        )
        .get_matches();

    // If launched as a file handler, only handle the file and exit
    if let Some(file_path) = matches.get_one::<String>("file") {
        info!("📁 Handling file: {}", file_path);
        return handle_file_path(file_path).await;
    }

    let _ = std::process::Command::new("fusermount")
        .arg("-u")
        .arg("~/OneDrive")
        .status();

    // Set panic hook for user notification
    std::panic::set_hook(Box::new(|panic_info| {
        let _ = std::process::Command::new("fusermount")
            .arg("-u")
            .arg("~/OneDrive")
            .status();

        let msg = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };
        error!("Panic: {:?}", msg);
        let _ = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new();
            if let Ok(rt) = rt {
                let _ = rt.block_on(async {
                    if let Ok(sender) = NotificationSender::new().await {
                        let _ = sender.send_notification(
                            "Open OneDrive",
                            0,
                            "dialog-error",
                            "Open OneDrive",
                            &format!("Open OneDrive experienced an unexpected error and has to shut down.\n\n{}", msg),
                            vec![],
                            vec![("urgency", &NotificationUrgency::Critical.to_u8().to_string())],
                            10000,
                        ).await;
                    }
                });
            }
        });
    }));

    // Create shutdown manager
    let shutdown_manager = ShutdownManager::new();
    let mut shutdown_rx = shutdown_manager.subscribe();

    // Initialize application
    let app = AppSetup::initialize().await?;
    app.authenticate().await?;
    app.setup_infrastructure().await?;
    app.setup_user_profile().await?;
    app.display_info();

    // Prepare FUSE mount directory
    let home_dir = std::env::var("HOME").expect("HOME not set");
    let mount_point = PathBuf::from(format!("{}/OneDrive", home_dir));
    if !mount_point.exists() {
        info!("Creating mount directory: {}", mount_point.display());
        fs::create_dir_all(&mount_point)?;
    }
    // Check if directory is empty
    if fs::read_dir(&mount_point)?.next().is_some() {
        error!("Mount directory {} is not empty", mount_point.display());
        return Err(anyhow::anyhow!(
            "Mount directory {} is not empty",
            mount_point.display()
        ));
    }

    // Prepare FUSE filesystem
    let pool = app.app_state.persistency().pool().clone();
    let download_queue_repo = DownloadQueueRepository::new(pool.clone());
    let fuse_fs = OneDriveFuse::new(
        pool.clone(),
        download_queue_repo,
        app.app_state.file_manager.clone(),
        app.app_state.clone(),
    )
    .await?;
    fuse_fs.initialize().await.ok();
    info!("✅ FUSE filesystem initialized successfully");

    // Start FUSE in a separate thread with shutdown handling
    let mount_point_for_mount = mount_point.clone();
    let mount_point_for_unmount = mount_point.clone();
    let (fuse_tx, fuse_rx) = std::sync::mpsc::channel();
    let mut fuse_shutdown_rx = shutdown_manager.subscribe();
    let fuse_handle = thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            // Start FUSE mount in a separate task
            let mount_task = tokio::spawn(async move {
                let result = fuser::mount2(
                    fuse_fs,
                    &mount_point_for_mount,
                    &[
                        MountOption::FSName("onedrive".to_string()),
                        MountOption::NoExec,
                        MountOption::NoSuid,
                        MountOption::NoDev,
                        MountOption::DefaultPermissions,
                        MountOption::NoAtime,
                        MountOption::CUSTOM("case_insensitive".to_string()),
                        
                    ],
                );
                if let Err(e) = result {
                    error!("FUSE mount error: {}", e);
                }
            });

            // Wait for shutdown signal
            let _ = fuse_shutdown_rx.recv().await;

            // Gracefully unmount FUSE
            #[cfg(target_os = "linux")]
            {
                let _ = std::process::Command::new("fusermount")
                    .arg("-u")
                    .arg(&mount_point_for_unmount)
                    .status();
            }
            #[cfg(target_os = "macos")]
            {
                let _ = std::process::Command::new("umount")
                    .arg(&mount_point_for_unmount)
                    .status();
            }

            // Cancel mount task
            mount_task.abort();
            let _ = fuse_tx.send(());
        });
    });

    // Start DBus server
    let mut dbus_server = crate::dbus_server::DbusServerManager::new(app.app_state.clone());
    if let Err(e) = dbus_server.start().await {
        error!("Failed to start DBus server: {}", e);
        // Continue without DBus server
    } else {
        info!("✅ DBus server started successfully");
    }

    // Start periodic sync scheduler with shutdown handling
    let sync_cycle = SyncCycle::new(app.app_state.clone());
    let mut scheduler = crate::scheduler::periodic_scheduler::PeriodicScheduler::new();
    let sync_task = sync_cycle.get_task().await?;
    scheduler.add_task(sync_task);

    let scheduler_shutdown_rx = shutdown_manager.subscribe();
    let scheduler_handle = tokio::spawn(async move {
        let mut shutdown_rx = scheduler_shutdown_rx;

        // Start the scheduler
        if let Err(e) = scheduler.start().await {
            error!("Failed to start scheduler: {}", e);
            return;
        }

        // Wait for shutdown signal
        let _ = shutdown_rx.recv().await;
        info!("🛑 Scheduler received shutdown signal");

        // Stop the scheduler
        scheduler.stop().await;
        info!("✅ Scheduler shutdown complete");
    });

    // Start signal handling
    let signal_shutdown_manager = shutdown_manager.clone();
    let signal_handle = tokio::spawn(async move {
        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("🛑 Received Ctrl+C signal");
                signal_shutdown_manager.shutdown().await;
            }
            _ = async {
                if let Ok(mut sigterm) = signal::unix::signal(signal::unix::SignalKind::terminate()) {
                    sigterm.recv().await;
                }
            } => {
                info!("🛑 Received SIGTERM signal");
                signal_shutdown_manager.shutdown().await;
            }
            _ = async {
                if let Ok(mut sigint) = signal::unix::signal(signal::unix::SignalKind::interrupt()) {
                    sigint.recv().await;
                }
            } => {
                info!("🛑 Received SIGINT signal");
                signal_shutdown_manager.shutdown().await;
            }
        }
    });

    info!("🟢 Open OneDrive is running. Press Ctrl+C to exit gracefully.");

    // Wait for shutdown signal
    let _ = shutdown_rx.recv().await;
    info!("🛑 Shutdown initiated...");

    // Stop DBus server
    if let Err(e) = dbus_server.stop().await {
        error!("Failed to stop DBus server: {}", e);
    }

    // Wait for all tasks to complete
    let _ = tokio::time::timeout(Duration::from_secs(30), async {
        let _ = scheduler_handle.await;
        let _ = signal_handle.await;
    })
    .await;

    // Wait for FUSE thread to finish
    let _ = fuse_rx.recv();
    let _ = fuse_handle.join();

    // Remove mount directory
    if mount_point.exists() {
        let _ = fs::remove_dir_all(&mount_point);
    }

    info!("👋 Open OneDrive exited cleanly.");
    Ok(())
}

/// Handle a file path when launched as a MIME type handler using DriveItemWithFuse
async fn handle_file_path(file_path: &str) -> Result<()> {
    info!("🚀 OneDrive file handler launched for: {}", file_path);

    // Initialize minimal app state for database access
    let app = AppSetup::initialize().await?;

    // Check if this is a .onedrivedownload file (new virtual file system)
    if file_path.ends_with(".onedrivedownload") {
        return handle_virtual_file(file_path, &app).await;
    }

    // Legacy JSON placeholder handling
    match parse_onedrive_file(file_path) {
        Ok((onedrive_id, virtual_path)) => {
            info!("📥 Queuing download for OneDrive ID: {}", onedrive_id);

            // Get DriveItemWithFuse from database using virtual path
            let pool = app.app_state.persistency().pool().clone();
            let drive_item_with_fuse_repo = crate::persistency::drive_item_with_fuse_repository::DriveItemWithFuseRepository::new(pool.clone());
            let download_queue_repo = DownloadQueueRepository::new(pool);

            // Try to find the item by virtual path first, then by OneDrive ID
            let item = if let Ok(Some(item)) = drive_item_with_fuse_repo
                .get_drive_item_with_fuse_by_virtual_path(&virtual_path)
                .await
            {
                Some(item)
            } else {
                drive_item_with_fuse_repo
                    .get_drive_item_with_fuse(&onedrive_id)
                    .await
                    .ok()
                    .flatten()
            };

            if let Some(item_with_fuse) = item {
                info!(
                    "📁 Found item: {} (OneDrive ID: {})",
                    item_with_fuse.name().unwrap_or("unnamed"),
                    item_with_fuse.id()
                );

                // Use file manager from app state
                let file_manager = app.app_state.file_manager();

                // Use the local path from the item if available, otherwise construct it
                let local_path = file_manager.get_download_dir().join(onedrive_id.clone());

                // Add to download queue
                download_queue_repo
                    .add_to_download_queue(&onedrive_id, &local_path)
                    .await?;

                info!("✅ Download queued successfully for: {}", virtual_path);
                info!("💾 Local path: {}", local_path.display());

                // Send desktop notification
                let notification_sender = NotificationSender::new().await;
                if let Ok(sender) = notification_sender {
                    let filename = item_with_fuse.name().unwrap_or("file");
                    let _ = sender
                        .send_notification(
                            "Open OneDrive",
                            0,
                            "cloud-upload",
                            "Open OneDrive",
                            &format!("File {} added to download queue", filename),
                            vec![],
                            vec![("urgency", &NotificationUrgency::Normal.to_u8().to_string())],
                            5000,
                        )
                        .await;
                }
            } else {
                warn!(
                    "⚠️ Item not found in database for OneDrive ID: {} or virtual path: {}",
                    onedrive_id, virtual_path
                );

                // Fallback: use the old approach
                let file_manager = app.app_state.file_manager();
                let local_path = file_manager.get_download_dir().join(onedrive_id.clone());
                download_queue_repo
                    .add_to_download_queue(&onedrive_id, &local_path)
                    .await?;

                info!(
                    "✅ Download queued using fallback method for: {}",
                    virtual_path
                );
            }
        }
        Err(e) => {
            error!("❌ Failed to parse OneDrive file: {}", e);
            return Err(e);
        }
    }

    Ok(())
}

/// Handle .onedrivedownload virtual files from the new FUSE system
async fn handle_virtual_file(file_path: &str, app: &AppSetup) -> Result<()> {
    info!("📁 Handling virtual file: {}", file_path);

    // Extract the virtual path from the file path
    // Example: /home/digit1024/OneDrive/Apps/Designer/file.txt.onedrivedownload
    // Should become: /Apps/Designer/file.txt
    let virtual_path = extract_virtual_path_from_file_path(file_path)?;
    info!("🔍 Looking for virtual path: {}", virtual_path);

    // Get database repositories
    let pool = app.app_state.persistency().pool().clone();
    let drive_item_with_fuse_repo =
        crate::persistency::drive_item_with_fuse_repository::DriveItemWithFuseRepository::new(
            pool.clone(),
        );
    let download_queue_repo = DownloadQueueRepository::new(pool);

    // Try to find the item by virtual path (much more efficient than loading all items)
    let item = drive_item_with_fuse_repo
        .get_drive_item_with_fuse_by_virtual_path(&virtual_path)
        .await?;

    if let Some(item_with_fuse) = item {
        let onedrive_id = item_with_fuse.id();
        let filename = item_with_fuse.name().unwrap_or("unknown");
        info!("📁 Found item: {} (OneDrive ID: {})", filename, onedrive_id);

        // Use file manager from app state
        let file_manager = app.app_state.file_manager();

        // Get the inode for this file to determine local path
        let local_path = if let Some(ino) = item_with_fuse.virtual_ino() {
            file_manager.get_download_dir().join(ino.to_string())
        } else {
            file_manager.get_download_dir().join(onedrive_id)
        };

        // Add to download queue
        download_queue_repo
            .add_to_download_queue(&onedrive_id, &local_path)
            .await?;

        info!("✅ Download queued successfully for: {}", filename);
        info!("💾 Local path: {}", local_path.display());

        // Send desktop notification
        let notification_sender = NotificationSender::new().await;
        if let Ok(sender) = notification_sender {
            let _ = sender
                .send_notification(
                    "Open OneDrive",
                    0,
                    "cloud-upload",
                    "Open OneDrive",
                    &format!("File {} added to download queue", filename),
                    vec![],
                    vec![("urgency", &NotificationUrgency::Normal.to_u8().to_string())],
                    5000,
                )
                .await;
        }
    } else {
        warn!(
            "⚠️ Item not found in database for virtual path: {}",
            virtual_path
        );

        // Send error notification
        let notification_sender = NotificationSender::new().await;
        if let Ok(sender) = notification_sender {
            let _ = sender
                .send_notification(
                    "Open OneDrive",
                    0,
                    "dialog-error",
                    "Open OneDrive",
                    &format!("File not found in OneDrive: {}", virtual_path),
                    vec![],
                    vec![(
                        "urgency",
                        &NotificationUrgency::Critical.to_u8().to_string(),
                    )],
                    5000,
                )
                .await;
        }

        return Err(anyhow::anyhow!(
            "File not found in OneDrive: {}",
            virtual_path
        ));
    }

    Ok(())
}

/// Extract virtual path from file path
///
/// Example:
/// Input: "/home/digit1024/OneDrive/Apps/Designer/file.txt.onedrivedownload"
/// Output: "/Apps/Designer/file.txt"
fn extract_virtual_path_from_file_path(file_path: &str) -> Result<String> {
    let path = std::path::Path::new(file_path);

    // Convert to string and find "OneDrive" in the path
    let path_str = path.to_string_lossy();
    let onedrive_index = path_str
        .find("OneDrive")
        .ok_or_else(|| anyhow::anyhow!("OneDrive directory not found in path: {}", file_path))?;

    // Extract everything after "OneDrive/"
    let after_onedrive = &path_str[onedrive_index + "OneDrive".len()..];

    // Remove .onedrivedownload suffix if present
    let virtual_path = if after_onedrive.ends_with(".onedrivedownload") {
        &after_onedrive[..after_onedrive.len() - ".onedrivedownload".len()]
    } else {
        after_onedrive
    };

    // Ensure it starts with "/"
    let virtual_path = if virtual_path.starts_with('/') {
        virtual_path.to_string()
    } else {
        format!("/{}", virtual_path)
    };

    Ok(virtual_path)
}

/// Parse a OneDrive file path to extract OneDrive ID and virtual path using DriveItemWithFuse
fn parse_onedrive_file(file_path: &str) -> Result<(String, String)> {
    // Read the JSON placeholder file
    let content = std::fs::read_to_string(file_path)?;
    let json: serde_json::Value = serde_json::from_str(&content)?;

    // Extract OneDrive ID
    let onedrive_id = json["onedrive_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing onedrive_id in file"))?
        .to_string();

    // Extract virtual path from JSON if available, otherwise reconstruct from file path
    let virtual_path = if let Some(path) = json["virtual_path"].as_str() {
        path.to_string()
    } else {
        // Fallback: reconstruct from file path
        format!(
            "/{}",
            std::path::Path::new(file_path)
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("Invalid file path"))?
                .to_string_lossy()
        )
    };

    Ok((onedrive_id, virtual_path))
}
