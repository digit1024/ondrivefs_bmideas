//! OneDrive FUSE filesystem for Linux
//!
//! This is a FUSE filesystem that provides access to OneDrive files
//! through a local mount point. Files are cached locally and synchronized
//! with OneDrive in the background.

mod app_state;
mod auth;
mod connectivity;
mod file_manager;
mod fuse_filesystem;
mod log_appender;
mod onedrive_service;
mod persistency;
mod scheduler;
mod tasks;

use std::sync::Arc;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Arg;
use clap::Command;
use log::{error, info};
use onedrive_sync_lib::notifications::NotificationSender;
use onedrive_sync_lib::notifications::NotificationUrgency;
use serde_json;

use crate::app_state::app_state_factory;
use crate::fuse_filesystem::OneDriveFuse;
use crate::log_appender::setup_logging;
use crate::persistency::download_queue_repository::DownloadQueueRepository;
use crate::persistency::fuse_repository::FuseRepository;
use crate::persistency::profile_repository::ProfileRepository;
use crate::tasks::delta_update::SyncCycle;
use fuser::MountOption;
use crate::file_manager::{DefaultFileManager, FileManager};


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
            Arg::new("mount")
                .long("mount")
                .short('m')
                .num_args(1)
                .help("Mount point for FUSE filesystem"),
        )
        .arg(
            Arg::new("file")
                .num_args(1)
                .help("File path to handle (for MIME type handler)"),
        )
        .get_matches();

    // Check if this is a MIME type handler call
    if let Some(file_path) = matches.get_one::<String>("file") {
        info!("📁 Handling file: {}", file_path);
        return handle_file_path(file_path).await;
    }

    // Initialize application
    let app = AppSetup::initialize().await?;

    // Authenticate with OneDrive
    app.authenticate().await?;

    // Setup infrastructure
    app.setup_infrastructure().await?;

    // Setup user profile
    app.setup_user_profile().await?;

    // Display application information
    app.display_info();

    // Check for mount argument
    if let Some(mount_point) = matches.get_one::<String>("mount") {
        info!("🔗 Mounting FUSE filesystem at {}", mount_point);
        // Prepare repositories
        let pool = app.app_state.persistency().pool().clone();
        let download_queue_repo = DownloadQueueRepository::new(pool.clone());
        let fuse_fs = OneDriveFuse::new_with_file_manager(
            pool, 
            download_queue_repo, 
            app.app_state.file_manager.clone()
        ).await?;
        fuse_fs.initialize().await.ok();
        info!("✅ FUSE filesystem initialized successfully");
        fuser::mount2(
            fuse_fs,
            mount_point,
            &[MountOption::FSName("onedrive".to_string())],
        )?;
        return Ok(());
    }

    // Start the main sync cycle
    app.start_sync_cycle().await?;

    info!("🎉 Daemon started successfully");
    Ok(())
}

/// Handle a file path when launched as a MIME type handler
async fn handle_file_path(file_path: &str) -> Result<()> {
    info!("🚀 OneDrive file handler launched for: {}", file_path);
    
    // Initialize minimal app state for database access
    let app = AppSetup::initialize().await?;
    
    // Parse the file path to extract OneDrive ID and virtual path
    // The file should be a JSON placeholder with OneDrive metadata
    match parse_onedrive_file(file_path) {
        Ok((onedrive_id, virtual_path)) => {
            info!("📥 Queuing download for OneDrive ID: {}", onedrive_id);
            
            // Queue the download
            let pool = app.app_state.persistency().pool().clone();
            let download_queue_repo = DownloadQueueRepository::new(pool);
            
            // Use file manager from app state
            let file_manager = app.app_state.file_manager();
            
            // Convert virtual path to local path
            let virtual_path_buf = PathBuf::from(virtual_path.clone());
            let local_path = file_manager.get_download_dir().join(onedrive_id.clone());
            
            // Add to download queue
            download_queue_repo.add_to_download_queue(&onedrive_id, &local_path).await?;
            
            info!("✅ Download queued successfully for: {}", virtual_path);
            info!("💾 Local path: {}", local_path.display());

            // Send desktop notification
            let notification_sender = NotificationSender::new().await;
            if let Ok(sender) = notification_sender {
                let filename = std::path::Path::new(file_path)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| "file".to_string());
                let _ = sender.send_notification(
                    "Open OneDrive",
                    0,
                    "cloud-upload",
                    "Open OneDrive",
                    &format!("File {} added to download queue", filename),
                    vec![],
                    vec![("urgency", &NotificationUrgency::Normal.to_u8().to_string())],
                    5000,
                ).await;
            }
        }
        Err(e) => {
            error!("❌ Failed to parse OneDrive file: {}", e);
            return Err(e);
        }
    }
    
    Ok(())
}

/// Parse a OneDrive file path to extract OneDrive ID and virtual path
fn parse_onedrive_file(file_path: &str) -> Result<(String, String)> {
    // Read the JSON placeholder file
    let content = std::fs::read_to_string(file_path)?;
    let json: serde_json::Value = serde_json::from_str(&content)?;
    
    // Extract OneDrive ID and virtual path
    let onedrive_id = json["onedrive_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing onedrive_id in file"))?
        .to_string();
    
    // For now, we'll use the file path to reconstruct the virtual path
    // In a real implementation, you might want to store the virtual path in the JSON
    let virtual_path = format!("/{}", std::path::Path::new(file_path)
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Invalid file path"))?
        .to_string_lossy());
    
    Ok((onedrive_id, virtual_path))
}
