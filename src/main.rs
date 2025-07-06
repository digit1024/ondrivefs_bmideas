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
use crate::openfs::opendrive_fuse::mount_filesystem_with_deps;
use crate::sync::sync_service::SyncService;
use anyhow::{Context, Result};
use clap::{Command, Arg};
use log::{error, info};
use std::os::unix::thread;
use std::path::Path;
use std::sync::Arc;

fn main() -> Result<()> {
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
        .get_matches();

    let mountpoint = matches.get_one::<String>("mountpoint").unwrap();
    let config_file = matches.get_one::<String>("config").unwrap();
    

    info!("Starting OneDrive FUSE filesystem");
    info!("Mount point: {}", mountpoint);
    info!("Config file: {}", config_file);

    // Always try to unmount on startup to ensure clean mounting
    // This handles both proper mounts and broken mounts like "Transport endpoint is not connected"
    info!("Ensuring clean mount point by unmounting any existing filesystem...");
    let _ = std::process::Command::new("fusermount")
        .arg("-u")
        .arg(mountpoint)
        .output(); // Ignore errors - mount might not exist, which is fine
    info!("Mount point cleanup completed");
    // Check if mountpoint exists and is a directory
    let mount_path = Path::new(mountpoint);
    if !mount_path.exists() {
        return Err(anyhow::anyhow!("Mount point does not exist: {}", mountpoint));
    }
    if !mount_path.is_dir() {
        return Err(anyhow::anyhow!("Mount point is not a directory: {}", mountpoint));
    }



    // Create runtime manually
    let runtime = tokio::runtime::Runtime::new()
        .context("Failed to create tokio runtime")?;

    // Get the runtime handle
    let runtime_handle = runtime.handle().clone();
    runtime.block_on(async {
    //check if serret exists - if not authenticate
    let auth = Arc::new(OneDriveAuth::new()?);
    let _token = auth.get_valid_token().await
        .context("Failed to get access token");
    if _token.is_err() {
        auth.authorize().await?;
    }
        
    Ok::<_, anyhow::Error>(())
    })?;
    std::thread::sleep(std::time::Duration::from_secs(10));


    // Run async setup in runtime
    let (file_manager, onedrive_client, mut sync_service) = runtime.block_on(async {
        // Load configuration
        let settings = Settings::load_from_file()
            .context("Failed to load settings")?;
        let config = SyncConfig::default();

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

        

        // Create file manager
        let file_manager = crate::file_manager::DefaultFileManager::new().await
            .context("Failed to create file manager")?;

        Ok::<_, anyhow::Error>((file_manager, client, sync_service))
    })?;

    // Mount the filesystem in a separate thread (FUSE is blocking)
    info!("Mounting filesystem at: {}", mountpoint);
    
    let mountpoint_clone = mountpoint.clone();
    let mountpoint_for_shutdown = mountpoint.clone();
    std::thread::spawn(move || {
        if let Err(e) = mount_filesystem_with_deps(&mountpoint_clone, file_manager, onedrive_client, runtime_handle) {
            error!("FUSE filesystem error: {}", e);
        }
    });
    let result = runtime.block_on(async {
        sync_service.init().await
            .context("Failed to initialize sync service")?;
        Ok::<_, anyhow::Error>(())
    });
    if let Err(e) = result {
        error!("Failed to initialize sync service: {}", e);
    }

    // Keep the runtime alive for any background tasks
    info!("FUSE filesystem mounted successfully. Press Ctrl+C to unmount.");
    
    // Wait for interrupt signal
    ctrlc::set_handler(move || {
        info!("Received interrupt signal, shutting down...");
        
        // Try to unmount the filesystem gracefully
        if let Err(e) = std::process::Command::new("fusermount")
            .arg("-u")
            .arg(&mountpoint_for_shutdown)
            .output() {
            error!("Failed to unmount filesystem: {}", e);
        } else {
            info!("Filesystem unmounted successfully");
        }
        
        std::process::exit(0);
    }).expect("Error setting Ctrl-C handler");

    // Keep main thread alive
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
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
