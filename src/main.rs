//! OneDrive FUSE filesystem for Linux
//! 
//! This is a FUSE filesystem that provides access to OneDrive files
//! through a local mount point. Files are cached locally and synchronized
//! with OneDrive in the background.

mod auth;
mod config;
mod file_manager;
mod helpers;
mod metadata_manager_for_files;
mod onedrive_service;
mod openfs;
mod operations;
mod sync;

use crate::auth::onedrive_auth::OneDriveAuth;
use crate::config::{Settings, SyncConfig};
use crate::onedrive_service::onedrive_client::OneDriveClient;
use crate::openfs::opendrive_fuse::mount_filesystem;
use crate::sync::sync_service::SyncService;
use anyhow::{Context, Result};
use clap::{Command, Arg};
use log::{error, info};
use std::path::Path;
use tokio::time::{sleep, Duration};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::init();

    // Parse command line arguments
    let matches = Command::new("OneDrive FUSE Filesystem")
        .version("1.0")
        .about("Mount OneDrive as a FUSE filesystem")
        .arg(
            Arg::new("mountpoint")
                .short('m')
                .long("mountpoint")
                .value_name("PATH")
                .help("Mount point path")
                .required(true)
                .num_args(1),
        )
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .value_name("FILE")
                .help("Configuration file path")
                .default_value("config.toml")
                .num_args(1),
        )
        .arg(
            Arg::new("daemon")
                .short('d')
                .long("daemon")
                .help("Run as daemon (background process)"),
        )
        .get_matches();

    let mountpoint = matches.get_one::<String>("mountpoint").unwrap();
    let config_file = matches.get_one::<String>("config").unwrap();
    let daemon_mode = matches.contains_id("daemon");

    info!("Starting OneDrive FUSE filesystem");
    info!("Mount point: {}", mountpoint);
    info!("Config file: {}", config_file);

    // Load configuration
    let settings = Settings::load_from_file()
        .context("Failed to load settings")?;
    let config = SyncConfig::default();

    // Check if mountpoint exists and is a directory
    let mount_path = Path::new(mountpoint);
    if !mount_path.exists() {
        return Err(anyhow::anyhow!("Mount point does not exist: {}", mountpoint));
    }
    if !mount_path.is_dir() {
        return Err(anyhow::anyhow!("Mount point is not a directory: {}", mountpoint));
    }

    // Initialize OneDrive authentication
    let auth = Arc::new(OneDriveAuth::new()?);
    
    // Get access token
    let _token = auth.get_valid_token().await
        .context("Failed to get access token")?;

    // Create OneDrive client
    let client = OneDriveClient::new(auth)
        .context("Failed to create OneDrive client")?;

    // Create sync service
    let mut sync_service = SyncService::new(
        client.clone(),
        config.clone(),
        settings.clone(),
    ).await
    .context("Failed to create sync service")?;

    // Initialize sync service
    sync_service.init().await
        .context("Failed to initialize sync service")?;

    if daemon_mode {
        info!("Running in daemon mode");
        
        // Start background sync loop
        let sync_client = client.clone();
        let sync_config = config.clone();
        let sync_settings = settings.clone();
        
        tokio::spawn(async move {
            loop {
                match SyncService::new(
                    sync_client.clone(),
                    sync_config.clone(),
                    sync_settings.clone(),
                ).await {
                    Ok(mut service) => {
                        if let Err(e) = service.update_cache().await {
                            error!("Sync error: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("Failed to create sync service: {}", e);
                    }
                }
                
                // Wait before next sync
                sleep(Duration::from_secs(300)).await; // 5 minutes
            }
        });
    }

    // Mount the filesystem
    info!("Mounting filesystem at: {}", mountpoint);
    mount_filesystem(mountpoint)
        .context("Failed to mount filesystem")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_mountpoint_validation() {
        // Test with non-existent path
        let result = std::panic::catch_unwind(|| {
            let mount_path = Path::new("/non/existent/path");
            if !mount_path.exists() {
                panic!("Mount point does not exist");
            }
        });
        assert!(result.is_err());

        // Test with existing directory
        let temp_dir = tempdir().unwrap();
        let mount_path = temp_dir.path();
        assert!(mount_path.exists());
        assert!(mount_path.is_dir());
    }
}
