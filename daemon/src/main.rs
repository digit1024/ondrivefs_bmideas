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
mod app_state;
mod tasks;

use anyhow::{Context, Result};
use clap::Command;
use log::{info, debug};
use onedrive_sync_lib::config::ProjectConfig;
use std::path::PathBuf;
use std::sync::Arc;

use crate::auth::onedrive_auth::OneDriveAuth;
use crate::log_appender::setup_logging;
use crate::onedrive_service::onedrive_client::OneDriveClient;
use crate::persistency::{PersistencyManager, database::{DriveItemRepository, SyncStateRepository, DownloadQueueRepository, UploadQueueRepository, ProfileRepository}};
use crate::onedrive_service::onedrive_models::{DriveItem, FolderFacet, FileFacet, ParentReference};
use crate::connectivity::{ConnectivityChecker, ConnectivityStatus};
use crate::app_state::{AppState, app_state_factory};



#[tokio::main]
async fn main() -> Result<()> {
    // Initialize project configuration

    let app_state = app_state_factory().await?;
    
    let project_config = ProjectConfig::new().await?;

    let auth = app_state.auth.clone();
    let load_result = auth.load_tokens();
    if load_result.is_err() {
        info!("No tokens found, will authorize");
        auth.authorize().await.context("Failed to authorize")?;
        auth.load_tokens().context("Failed to load tokens")?;
        panic!("Authorization failed - panicking");

    }

    info!("Tokens loaded successfully");
    

    

    // Initialize logging
    setup_logging(&project_config.project_dirs.data_dir().to_path_buf())
        .await
        .context("Failed to setup logging")?;

    let db = app_state.persistency_manager.clone();
    // Initialize database schema ( if not exists)
    db.init_database().await
        .context("Failed to initialize database schema")?;
    
    // Initialize connectivity checker
    let connectivity_checker = app_state.connectivity_checker.clone();
    
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
    
    
    
    // DEMO: Test profile fetching functionality
    info!("👤 Starting profile fetching demo...");
    
    // Create profile repository
    let profile_repo = ProfileRepository::new(db.pool().clone());
    
    // Try to get existing profile from database
    match profile_repo.get_profile().await {
        Ok(Some(profile)) => {
            info!("📋 Found stored profile: {} ({})", 
                profile.display_name.as_deref().unwrap_or("Unknown"),
                profile.mail.as_deref().unwrap_or("No email"));
        }
        Ok(None) => {
            info!("📋 No stored profile found, will fetch from API when authenticated");
            info!("Autthenitcating now");

            
            let onedrive_client = app_state.onedrive_client.clone();

            let profile = onedrive_client.get_user_profile().await.context("Failed to get user profile")?;
            profile_repo.store_profile(&profile).await.context("Failed to store profile")?;

            
        }
        Err(e) => {
            info!("⚠️ Error retrieving stored profile: {}", e);
        }
    }
    
        info!("✅ Profile fetching demo completed!");
    
    // Example function to fetch and store profile (commented out since we need auth)
    // async fn fetch_and_store_profile(onedrive_client: &OneDriveClient, profile_repo: &ProfileRepository) -> Result<()> {
    //     info!("🔄 Fetching user profile from Microsoft Graph...");
    //     
    //     let profile = onedrive_client.get_user_profile().await?;
    //     profile_repo.store_profile(&profile).await?;
    //     
    //     info!("✅ Profile fetched and stored: {} ({})", 
    //         profile.display_name.as_deref().unwrap_or("Unknown"),
    //         profile.mail.as_deref().unwrap_or("No email"));
    //     
    //     Ok(())
    // }
    
    // Parse command line arguments
    let _matches = Command::new("OneDrive Client for Linux by digit1024@github")
        .version("01.0")
        .about("Mount OneDrive as a FUSE filesystem")
        .get_matches();

    info!("Daemon started with persistency manager initialized");
    info!("Database location: {}", app_state.persistency_manager.db_path().display());
    
    Ok(())
}


