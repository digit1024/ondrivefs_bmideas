mod auth;
mod helpers;
mod openfs;
use anyhow::Result;
use clap::{Arg, Command};
use log::{error, info, warn};
use openfs::opendrive_fuse;

mod onedrive_service;

mod config;
mod file_manager;
mod metadata_manager_for_files;

use auth::onedrive_auth::OneDriveAuth;
use config::{Settings, SyncConfig};
use file_manager::FileManager;
use onedrive_service::onedrive_client::OneDriveClient;

mod sync_service;
use sync_service::SyncService;


#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let matches = Command::new("OneDrive Sync")
        .version("1.0")
        .about("OneDrive synchronization daemon for Linux")
        .arg(
            Arg::new("fuse")
                .short('f')
                .long("fuse")
                .action(clap::ArgAction::SetTrue)
                .help("Fuse the local directory with the OneDrive directory"),
        )
        .arg(
            Arg::new("sync-daemon")
                .short('d')
                .long("sync-daemon")
                .action(clap::ArgAction::SetTrue)
                .help("Run sync daemon in background without FUSE mount"),
        )
        .arg(
            Arg::new("auth")
                .long("auth")
                .action(clap::ArgAction::SetTrue)
                .help("Run authorization flow only"),
        )
        .arg(
            Arg::new("list")
                .long("list")
                .action(clap::ArgAction::SetTrue)
                .help("List files in OneDrive root"),
        )
        .arg(
            Arg::new("list-dir")
                .long("list-dir")
                .value_name("PATH")
                .help("List files in a specific OneDrive directory"),
        )
        .arg(
            Arg::new("get-file")
                .long("get-file")
                .num_args(2)
                .value_names(["REMOTE_PATH", "LOCAL_PATH"])
                .help("Download a file from OneDrive to a local path"),
        )
        .arg(
            Arg::new("put-file")
                .long("put-file")
                .num_args(2)
                .value_names(["LOCAL_PATH", "REMOTE_PATH"])
                .help("Upload a local file to a OneDrive directory"),
        )
        .arg(
            Arg::new("settings-add-folder-to-sync")
                .long("settings-add-folder-to-sync")
                .value_name("FOLDER")
                .help("Add a folder to the sync list in settings.json"),
        )
        .arg(
            Arg::new("settings-remove-folder-to-sync")
                .long("settings-remove-folder-to-sync")
                .value_name("FOLDER")
                .help("Remove a folder from the sync list in settings.json"),
        )
        .arg(
            Arg::new("settings-list-folders-to-sync")
                .long("settings-list-folders-to-sync")
                .help("List all folders currently set to sync in settings.json")
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches();

    let mut config = SyncConfig::default();

    // Handle different modes
    if matches.get_flag("auth") {
        // Authorization only
        let auth = OneDriveAuth::new()?;
        auth.authorize().await?;
        println!("Authorization completed!");
        return Ok(());
    }

    if matches.get_flag("fuse") {
        let client = OneDriveClient::new()?;
        let settings = Settings::load_from_file()?;
        let mut daemon = SyncService::new(client, config.clone(), settings.clone()).await?;
        
        // Initial sync
        daemon.init().await?;

        //make sure the directory exists
        let path = config.local_dir.clone();
        if !path.exists() {
            std::fs::create_dir_all(path.clone())?;
        }

        // Start background sync task
        let sync_interval = config.sync_interval;
        let mut sync_daemon = daemon;
        let sync_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(sync_interval);
            interval.tick().await; // Skip first tick since we just did init()
            
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        info!("Starting periodic sync...");
                        if let Err(e) = sync_daemon.update_cache().await {
                            error!("Sync error: {}", e);
                        } else {
                            info!("Periodic sync completed successfully");
                        }
                    }
                    _ = tokio::signal::ctrl_c() => {
                        warn!("Sync daemon received shutdown signal, stopping...");
                        break;
                    }
                }
            }
        });

        // Setup signal handler for graceful shutdown
        let mountpoint = path.display().to_string();
        let mountpoint_for_signal = mountpoint.clone();
        
        // Start FUSE mount in a separate blocking task
        let fuse_handle = tokio::task::spawn_blocking(move || {
            opendrive_fuse::mount_filesystem(&mountpoint)
        });
        
        // Wait for Ctrl+C and handle shutdown
        tokio::select! {
            result = fuse_handle => {
                info!("FUSE filesystem has been unmounted");
                if let Err(e) = result? {
                    error!("FUSE mount failed: {}", e);
                }
            }
            _ = tokio::signal::ctrl_c() => {
                warn!("Received shutdown signal, stopping sync daemon and unmounting...");
                
                // Stop the sync daemon
                sync_handle.abort();
                
                // Unmount the filesystem
                info!("Attempting to unmount filesystem at: {}", mountpoint_for_signal);
                match std::process::Command::new("fusermount")
                    .arg("-u")
                    .arg(&mountpoint_for_signal)
                    .output() 
                {
                    Ok(output) => {
                        if output.status.success() {
                            info!("Successfully unmounted filesystem");
                        } else {
                            error!("Failed to unmount filesystem: {}", 
                                   String::from_utf8_lossy(&output.stderr));
                        }
                    }
                    Err(e) => {
                        error!("Failed to execute fusermount command: {}", e);
                    }
                }
                
                info!("Shutdown complete");
            }
        }

        info!("Sync daemon shutdown complete");
        return Ok(());
    }

    if matches.get_flag("sync-daemon") {
        let client = OneDriveClient::new()?;
        let settings = Settings::load_from_file()?;
        let mut daemon = SyncService::new(client, config.clone(), settings.clone()).await?;
        
        // Initial sync
        daemon.init().await?;
        info!("Initial sync completed. Starting periodic sync daemon...");
        info!("Sync interval: {:?}", config.sync_interval);

        // Run sync daemon in foreground
        let mut interval = tokio::time::interval(config.sync_interval);
        interval.tick().await; // Skip first tick since we just did init()
        
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    info!("Starting periodic sync...");
                    if let Err(e) = daemon.update_cache().await {
                        error!("Sync error: {}", e);
                    } else {
                        info!("Periodic sync completed successfully");
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    warn!("Received shutdown signal, stopping sync daemon...");
                    break;
                }
            }
        }

        return Ok(());
    }

    if let Some(get_args) = matches.get_many::<String>("get-file") {
        let args: Vec<_> = get_args.collect();
        let remote_path = &args[0];
        let local_path = &args[1];
        let client = OneDriveClient::new()?;
        let item = client.get_item_by_path(remote_path).await?;
        if let Some(download_url) = &item.download_url {
            let download_result = client
                .download_file(
                    download_url,
                    &item.id,
                    item.name.as_ref().map_or("Unknown", |v| v),
                )
                .await?;

            // Create file manager to handle the file save
            let metadata_manager =
                crate::metadata_manager_for_files::MetadataManagerForFiles::new()?;
            let file_manager = crate::file_manager::DefaultFileManager::new().await?;
            file_manager
                .save_downloaded_file_r(&download_result, std::path::Path::new(local_path))
                .await?;

            println!("Downloaded '{}' to '{}'", remote_path, local_path);
        } else {
            println!(
                "No download URL found for '{}'. Is it a folder?",
                remote_path
            );
        }
        return Ok(());
    }

    if let Some(put_args) = matches.get_many::<String>("put-file") {
        let args: Vec<_> = put_args.collect();
        let local_path = &args[0];
        let remote_path = &args[1];
        let client = OneDriveClient::new()?;

        // Read file data
        let file_data = tokio::fs::read(local_path).await?;
        let file_name = std::path::Path::new(local_path)
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("Invalid file path"))?
            .to_string_lossy();

        let upload_result = client
            .upload_file(&file_data, &file_name, remote_path)
            .await?;
        println!(
            "Uploaded '{}' to '{}' (ID: {})",
            local_path, remote_path, upload_result.onedrive_id
        );
        return Ok(());
    }

    // Handle settings management options
    if let Some(folder) = matches.get_one::<String>("settings-add-folder-to-sync") {
        let mut settings = Settings::load_from_file()?;
        if !settings.sync_folders.contains(folder) {
            settings.sync_folders.push(folder.clone());
            settings.save_to_file()?;
            println!("Added '{}' to sync folders.", folder);
        } else {
            println!("'{}' is already in the sync folders list.", folder);
        }
        return Ok(());
    }
    if let Some(folder) = matches.get_one::<String>("settings-remove-folder-to-sync") {
        let mut settings = Settings::load_from_file()?;
        if let Some(pos) = settings.sync_folders.iter().position(|f| f == folder) {
            settings.sync_folders.remove(pos);
            settings.save_to_file()?;
            println!("Removed '{}' from sync folders.", folder);
        } else {
            println!("'{}' was not found in the sync folders list.", folder);
        }
        return Ok(());
    }

    let settings = Settings::load_from_file()?;
    let client = OneDriveClient::new()?;

    Ok(())
}
