//! OneDrive FUSE filesystem for Linux
//!
//! This is a FUSE filesystem that provides access to OneDrive files
//! through a local mount point. Files are cached locally and synchronized
//! with OneDrive in the background.

mod auth;
mod connectivity;
mod log_appender;
mod onedrive_service;
mod persistency;
mod scheduler;

use anyhow::{Context, Result};
use clap::Command;
use log::{info, debug};
use onedrive_sync_lib::config::ProjectConfig;
use std::path::PathBuf;

use crate::log_appender::setup_logging;
use crate::persistency::{PersistencyManager, database::{DriveItemRepository, SyncStateRepository, DownloadQueueRepository, UploadQueueRepository}};
use crate::onedrive_service::onedrive_models::{DriveItem, FolderFacet, FileFacet, ParentReference};
use crate::connectivity::{ConnectivityChecker, ConnectivityStatus};

struct AppState {
    project_config: ProjectConfig,
    persistency_manager: PersistencyManager,
    connectivity_checker: ConnectivityChecker,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize project configuration
    let project_config = ProjectConfig::new().await?;
    
    // Initialize logging
    setup_logging(&project_config.project_dirs.data_dir().to_path_buf())
        .await
        .context("Failed to setup logging")?;

    // Initialize persistency manager
    let persistency_manager = PersistencyManager::new(
        project_config.project_dirs.data_dir().to_path_buf()
    ).await.context("Failed to initialize persistency manager")?;
    
    // Initialize database schema ( if not exists)
    persistency_manager.init_database().await
        .context("Failed to initialize database schema")?;
    
    // Initialize connectivity checker
    let connectivity_checker = ConnectivityChecker::new();
    
    // DEMO: Test connectivity checker functionality
    info!("🚀 Starting connectivity checker demo...");
    
    // Test basic connectivity
    let status = connectivity_checker.check_connectivity().await;
    info!("📡 Connectivity Status: {}", status);
    
    // Test detailed status
    let (detailed_status, details) = connectivity_checker.get_detailed_status().await;
    info!("📊 Detailed Status: {} - {}", detailed_status, details);
    
    // Test with different timeout
    let fast_checker = ConnectivityChecker::with_timeout(3);
    let fast_status = fast_checker.check_connectivity().await;
    info!("⚡ Fast Check Status: {}", fast_status);
    
    info!("✅ Connectivity checker demo completed!");
    
    let app_state = AppState {
        project_config,
        persistency_manager,
        connectivity_checker,
    };

    // Parse command line arguments
    let _matches = Command::new("OneDrive Client for Linux by digit1024@github")
        .version("01.0")
        .about("Mount OneDrive as a FUSE filesystem")
        .get_matches();

    info!("Daemon started with persistency manager initialized");
    info!("Database location: {}", app_state.persistency_manager.db_path().display());
    
    Ok(())
}


