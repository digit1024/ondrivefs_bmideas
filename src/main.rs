use anyhow::Result;
use clap::{Arg, Command};
use log::{info, error, warn};
use std::{path::PathBuf, str::FromStr};
use std::time::Duration;
use tokio::time::sleep;
use tokio::signal;

mod onedrive_auth;
mod onedrive_client;
mod token_store;
mod config;
mod metadata_manager_for_files;
use log::debug;

use onedrive_auth::OneDriveAuth;
use onedrive_client::OneDriveClient;
use config::{Settings, SyncConfig};


struct SyncDaemon {
    client: OneDriveClient,
    config: SyncConfig,
    settings: Settings,
}

impl SyncDaemon {
    fn new(config: SyncConfig, settings: Settings) -> Result<Self> {
        Ok(Self {
            client: OneDriveClient::new()?,
            config,
            settings,
        })
    }

    /// Run initial authorization if needed
    async fn ensure_authorized(&self) -> Result<()> {
        match self.client.list_root().await {
            Ok(_) => {
                info!("Already authorized and tokens are valid");
                Ok(())
            }
            Err(_) => {
                warn!("Authorization needed or tokens expired");
                // This will trigger the OAuth flow
                let auth = OneDriveAuth::new()?;
                auth.get_valid_token().await?;
                info!("Authorization completed");
                Ok(())
            }
        }
    }

    /// Extract delta token from delta link URL
    fn extract_delta_token(delta_link: &str) -> Option<String> {
        if let Some(token_start) = delta_link.find("token=") {
            let token = &delta_link[token_start + 6..];
            Some(token.to_string())
        } else {
            None
        }
    }

    /// Sync files from OneDrive to local directory using delta queries
    async fn sync_from_remote(&mut self) -> Result<()> {
        info!("Starting sync from remote using delta queries...");
        
        for folder in &self.settings.sync_folders {
            info!("Processing folder: {}", folder);
            
            // Get delta token for this folder from metadata manager
            let delta_token = self.client.metadata_manager().get_folder_delta(folder)?;
            
            // Get changes using delta query
            let changes = if let Some(delta) = delta_token {
                info!("Delta token found for folder: {}", folder);
                self.client.get_delta_with_token(folder, &delta.delta_token).await?
            } else {
                // Initial sync - get all files
                info!("Initial sync for folder: {}", folder);
                self.client.get_initial_delta(folder).await?
            };

            // Process changes
            for item in &changes.value {
                if item.deleted.is_some() {
                    // Handle deleted files
                    let stored_local_path = self.client.metadata_manager().get_local_path_from_one_drive_id(&item.id).unwrap().unwrap();
                    let local_path = PathBuf::from_str(&stored_local_path).unwrap();
                    
                    if local_path.exists() {
                        if let Err(e) = std::fs::remove_file(&local_path) {
                            error!("Failed to delete local file {:?}: {}", local_path, e);
                        } else {
                            info!("Deleted local file: {:?}", local_path);
                        }
                    }
                } else if item.file.is_some() {
                    // Handle new/updated files
                    let local_path = self.get_local_path_for_item(folder, item);
                    
                    // Create parent directory if it doesn't exist
                    if let Some(parent) = local_path.parent() {
                        if let Err(e) = tokio::fs::create_dir_all(parent).await {
                            error!("Failed to create directory {:?}: {}", parent, e);
                            continue;
                        }
                    }
                    // Get the full item metadata including download URL
                    let remote_path = format!("{}/{}", folder, item.name.as_ref().map_or("Unknown", |v| v));
                    match self.client.get_item_by_path(&remote_path).await {
                        Ok(full_item) => {
                            if let Some(download_url) = &full_item.download_url {
                                match self.client.download_file(download_url, &local_path, &item.id, item.name.as_ref().map_or("Unknown", |v| v)).await {
                                    Ok(_) => info!("Downloaded: {} -> {:?}", item.name.as_ref().map_or("Unknown", |v| v), local_path),
                                    Err(e) => error!("Failed to download {}: {}", item.name.as_ref().map_or("Unknown", |v| v), e),
                                }
                            } else {
                                error!("No download URL available for file: {}", item.name.as_ref().map_or("Unknown", |v| v));
                            }
                        }
                        Err(e) => {
                            error!("Failed to get download URL for {}: {}", item.name.as_ref().map_or("Unknown", |v| v), e);
                        }
                    }
                }
            }

            // Save delta token for next sync
            if let Some(delta_link) = &changes.delta_link {
                if let Some(token) = Self::extract_delta_token(delta_link) {
                    self.client.metadata_manager().store_folder_delta(folder, &token)?;
                    info!("Saved delta token for folder: {}", folder);
                }
            }
        }
        self.client.metadata_manager().flush()?;
        Ok(())
    }

    /// Get local path for a OneDrive item
    fn get_local_path_for_item(&self, folder: &str, item: &onedrive_client::DriveItem) -> PathBuf {
        let mut local_path = self.config.local_dir.clone();
        
        // Add the folder path (without leading slash)
        if folder != "/" {
            let folder_path = folder.trim_start_matches('/');
            local_path.push(folder_path);
        }
        if item.name.is_none() {
            panic!("Item name is missing for item with ID: {}", item.id)
        }else{
        // Add the item name
            local_path.push(item.clone().name.as_ref().unwrap());
        }
        local_path
    }

    /// Sync files from local directory to OneDrive (simplified for now)
    async fn sync_to_remote(&self) -> Result<()> {
        info!("Starting sync to remote...");
        // TODO: Implement proper local-to-remote sync using changed queue
        // For now, this is a placeholder
        Ok(())
    }

    /// Run a single sync cycle
    async fn sync_cycle(&mut self) -> Result<()> {
        info!("Starting sync cycle");
        
        // Sync from remote using delta queries
        if let Err(e) = self.sync_from_remote().await {
            error!("Failed to sync from remote: {}", e);
        }

        // TODO: Implement local-to-remote sync
        // if let Err(e) = self.sync_to_remote().await {
        //     error!("Failed to sync to remote: {}", e);
        // }

        info!("Sync cycle completed");
        Ok(())
    }

    /// Wait for shutdown signals (Ctrl+C or SIGTERM)
    async fn wait_for_shutdown() {
        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("Received Ctrl+C");
            }
            _ = async {
                let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
                    .expect("Failed to create SIGTERM signal handler");
                sigterm.recv().await;
            } => {
                info!("Received SIGTERM");
            }
        }
    }

    /// Run daemon mode
    async fn run_daemon(&mut self) -> Result<()> {
        info!("Starting OneDrive sync daemon");
        info!("Local directory: {:?}", self.config.local_dir);
        info!("Sync folders: {:?}", self.settings.sync_folders);
        info!("Sync interval: {:?}", self.config.sync_interval);
        info!("Press Ctrl+C or send SIGTERM to stop the daemon gracefully");

        self.ensure_authorized().await?;

        loop {
            // Use tokio::select! to handle both sync and shutdown signals
            tokio::select! {
                // Handle shutdown signals
                _ = Self::wait_for_shutdown() => {
                    info!("Shutting down gracefully...");
                    break;
                }
                // Run sync cycle with timeout
                _ = async {
                    if let Err(e) = self.sync_cycle().await {
                        error!("Sync cycle failed: {}", e);
                    }
                    sleep(self.config.sync_interval).await;
                } => {
                    // Sync cycle completed, continue to next iteration
                }
            }
        }

        info!("Daemon stopped gracefully");
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let matches = Command::new("OneDrive Sync")
        .version("1.0")
        .about("OneDrive synchronization daemon for Linux")
        .arg(
            Arg::new("daemon")
                .short('d')
                .long("daemon")
                .action(clap::ArgAction::SetTrue)
                .help("Run in daemon mode")
        )
        .arg(
            Arg::new("local-dir")
                .short('l')
                .long("local-dir")
                .value_name("PATH")
                .help("Local directory to sync")
        )
        .arg(
            Arg::new("remote-dir")
                .short('r')
                .long("remote-dir")
                .value_name("PATH")
                .help("Remote directory to sync")
        )
        .arg(
            Arg::new("interval")
                .short('i')
                .long("interval")
                .value_name("SECONDS")
                .help("Sync interval in seconds")
                .default_value("15")
        )
        .arg(
            Arg::new("auth")
                .long("auth")
                .action(clap::ArgAction::SetTrue)
                .help("Run authorization flow only")
        )
        .arg(
            Arg::new("list")
                .long("list")
                .action(clap::ArgAction::SetTrue)
                .help("List files in OneDrive root")
        )
        .arg(
            Arg::new("list-dir")
                .long("list-dir")
                .value_name("PATH")
                .help("List files in a specific OneDrive directory")
        )
        .arg(
            Arg::new("get-file")
                .long("get-file")
                .num_args(2)
                .value_names(["REMOTE_PATH", "LOCAL_PATH"])
                .help("Download a file from OneDrive to a local path")
        )
        .arg(
            Arg::new("put-file")
                .long("put-file")
                .num_args(2)
                .value_names(["LOCAL_PATH", "REMOTE_PATH"])
                .help("Upload a local file to a OneDrive directory")
        )
        .arg(
            Arg::new("settings-add-folder-to-sync")
                .long("settings-add-folder-to-sync")
                .value_name("FOLDER")
                .help("Add a folder to the sync list in settings.json")
        )
        .arg(
            Arg::new("settings-remove-folder-to-sync")
                .long("settings-remove-folder-to-sync")
                .value_name("FOLDER")
                .help("Remove a folder from the sync list in settings.json")
        )
        .arg(
            Arg::new("settings-list-folders-to-sync")
                .long("settings-list-folders-to-sync")
                .help("List all folders currently set to sync in settings.json")
                .action(clap::ArgAction::SetTrue)
        )
        .get_matches();

    let mut config = SyncConfig::default();

    if let Some(local_dir) = matches.get_one::<String>("local-dir") {
        config.local_dir = PathBuf::from(local_dir);
    }

    if let Some(remote_dir) = matches.get_one::<String>("remote-dir") {
        config.remote_dir = remote_dir.clone();
    }

    if let Some(interval) = matches.get_one::<String>("interval") {
        config.sync_interval = Duration::from_secs(interval.parse()?);
    }

    // Handle different modes
    if matches.get_flag("auth") {
        // Authorization only
        let auth = OneDriveAuth::new()?;
        auth.authorize().await?;
        println!("Authorization completed!");
        return Ok(());
    }
    if matches.get_flag("settings-list-folders-to-sync") {
        let settings = Settings::load()?;
        println!("Folders set to sync:");
        for folder in &settings.sync_folders {
            println!("- {}", folder);
        }
        return Ok(());
    }

    if matches.get_flag("list") {
        // List files only
        let client = OneDriveClient::new()?;
        let items = client.list_root().await?;
        
        println!("Files in OneDrive root:");
        for item in items {
            let type_str = if item.folder.is_some() { "📁" } else { "📄" };
            println!("{} {} ({})", type_str, item.name.unwrap_or("Unknown".to_string()), item.last_modified.unwrap_or("Unknown".to_string()));
        }
        return Ok(());
    }

    if let Some(list_dir) = matches.get_one::<String>("list-dir") {
        // List files in a specific OneDrive directory
        let client = OneDriveClient::new()?;
        let items = client.list_folder_by_path(list_dir).await?;
        println!("Files in OneDrive directory '{}':", list_dir);
        for item in items {
            let type_str = if item.folder.is_some() { "📁" } else { "📄" };
            println!("{} {} ({})", type_str, item.name.unwrap_or("Unknown".to_string()), item.last_modified.unwrap_or("Unknown".to_string()));
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
            client.download_file(download_url, std::path::Path::new(local_path), &item.id, item.name.as_ref().map_or("Unknown", |v| v)).await?;
            println!("Downloaded '{}' to '{}'", remote_path, local_path);
        } else {
            println!("No download URL found for '{}'. Is it a folder?", remote_path);
        }
        return Ok(());
    }

    if let Some(put_args) = matches.get_many::<String>("put-file") {
        let args: Vec<_> = put_args.collect();
        let local_path = &args[0];
        let remote_path = &args[1];
        let client = OneDriveClient::new()?;
        client.upload_file(std::path::Path::new(local_path), remote_path).await?;
        println!("Uploaded '{}' to '{}'", local_path, remote_path);
        return Ok(());
    }

    // Handle settings management options
    if let Some(folder) = matches.get_one::<String>("settings-add-folder-to-sync") {
        let mut settings = Settings::load()?;
        if !settings.sync_folders.contains(folder) {
            settings.sync_folders.push(folder.clone());
            settings.save()?;
            println!("Added '{}' to sync folders.", folder);
        } else {
            println!("'{}' is already in the sync folders list.", folder);
        }
        return Ok(());
    }
    if let Some(folder) = matches.get_one::<String>("settings-remove-folder-to-sync") {
        let mut settings = Settings::load()?;
        if let Some(pos) = settings.sync_folders.iter().position(|f| f == folder) {
            settings.sync_folders.remove(pos);
            settings.save()?;
            println!("Removed '{}' from sync folders.", folder);
        } else {
            println!("'{}' was not found in the sync folders list.", folder);
        }
        return Ok(());
    }


    let settings = Settings::load()?;
    if matches.get_flag("daemon") {
        let mut daemon = SyncDaemon::new(config, settings)?;
        daemon.run_daemon().await?;
    } else {
        let mut daemon = SyncDaemon::new(config, settings)?;
        daemon.ensure_authorized().await?;
        daemon.sync_cycle().await?;
        println!("Sync completed!");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_config_default() {
        let config = SyncConfig::default();
        assert_eq!(config.local_dir, PathBuf::from("./sync"));
        assert_eq!(config.remote_dir, "/sync");
        assert_eq!(config.sync_interval, Duration::from_secs(300));
    }
}