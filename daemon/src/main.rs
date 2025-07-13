//! OneDrive FUSE filesystem for Linux
//!
//! This is a FUSE filesystem that provides access to OneDrive files
//! through a local mount point. Files are cached locally and synchronized
//! with OneDrive in the background.

mod app_state;
mod auth;
mod connectivity;
mod fuse_filesystem;
mod log_appender;
mod onedrive_service;
mod persistency;
mod scheduler;
mod tasks;

use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Arg;
use clap::Command;
use log::{error, info};

use crate::app_state::app_state_factory;
use crate::fuse_filesystem::OneDriveFuse;
use crate::log_appender::setup_logging;
use crate::persistency::database::{DownloadQueueRepository, FuseRepository, ProfileRepository};
use crate::tasks::delta_update::SyncCycle;
use fuser::MountOption;

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
        .about("Mount OneDrive as a FUSE filesystem")
        .arg(
            Arg::new("mount")
                .long("mount")
                .short('m')
                .num_args(1)
                .help("Mount point for FUSE filesystem"),
        )
        .get_matches();

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
        let fuse_repo = FuseRepository::new(pool.clone());
        let download_queue_repo = DownloadQueueRepository::new(pool);
        let fuse_fs = OneDriveFuse::new(fuse_repo, download_queue_repo);
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
